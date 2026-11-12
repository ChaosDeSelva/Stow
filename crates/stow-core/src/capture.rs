//! Turn raw pasted/captured text into a canonical URL and classify it.

use url::Url;

use crate::models::ItemKind;
use crate::{Result, StowError};

const TRACKING_KEYS: &[&str] = &[
    "fbclid",
    "gclid",
    "dclid",
    "msclkid",
    "mc_cid",
    "mc_eid",
    "igshid",
    "ref",
    "ref_src",
    "si",
    "feature",
    "t_src",
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "utm_id",
];

const VIDEO_HOSTS: &[&str] = &[
    "youtube.com",
    "youtu.be",
    "vimeo.com",
    "loom.com",
    "twitch.tv",
];

fn is_tracking_key(host: &str, k: &str) -> bool {
    if (host_matches(host, "x.com") || host_matches(host, "twitter.com")) && matches!(k, "s" | "t")
    {
        return true;
    }
    k.starts_with("utm_") || TRACKING_KEYS.contains(&k)
}

fn host_matches(host: &str, base: &str) -> bool {
    host == base || host.ends_with(&format!(".{base}"))
}

/// Trim, require http(s), lowercase scheme/host, drop fragment and tracking params,
/// canonicalize `YouTube` URLs to `https://www.youtube.com/watch?v=ID[&t=..]`.
pub fn normalize_url(raw: &str) -> Result<Url> {
    let trimmed = raw.trim();
    let invalid = |reason: String| StowError::InvalidUrl {
        input: trimmed.to_string(),
        reason,
    };
    let mut url = Url::parse(trimmed).map_err(|e| invalid(e.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(invalid("scheme must be http or https".into()));
    }
    let host = url
        .host_str()
        .ok_or_else(|| invalid("missing host".into()))?
        .to_ascii_lowercase();
    url.set_host(Some(&host))
        .map_err(|e| invalid(e.to_string()))?;
    url.set_fragment(None);

    if let Some(yt) = youtube_canonical(&url) {
        return Ok(yt);
    }

    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(k, _)| !is_tracking_key(&host, k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    url.set_query(None);
    if !kept.is_empty() {
        let mut qp = url.query_pairs_mut();
        for (k, v) in &kept {
            qp.append_pair(k, v);
        }
    }
    Ok(url)
}

fn youtube_canonical(url: &Url) -> Option<Url> {
    let host = url.host_str()?;
    let id: Option<String> = if host_matches(host, "youtu.be") {
        url.path_segments()?
            .next()
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    } else if host_matches(host, "youtube.com") {
        let mut segs = url.path_segments()?;
        match segs.next() {
            Some("watch") => url
                .query_pairs()
                .find(|(k, _)| k == "v")
                .map(|(_, v)| v.into_owned()),
            Some("shorts" | "embed" | "live") => {
                segs.next().filter(|s| !s.is_empty()).map(str::to_string)
            }
            _ => None,
        }
    } else {
        return None;
    };
    let id = id?;
    let t = url
        .query_pairs()
        .find(|(k, _)| k == "t")
        .map(|(_, v)| v.into_owned());
    let mut out = Url::parse("https://www.youtube.com/watch").ok()?;
    {
        let mut qp = out.query_pairs_mut();
        qp.append_pair("v", &id);
        if let Some(t) = t {
            qp.append_pair("t", &t);
        }
    }
    Some(out)
}

/// Video hosts (`YouTube`, Vimeo, Loom, Twitch) → `Video`; everything else → `Article`.
pub fn classify_kind(url: &Url) -> ItemKind {
    match url.host_str() {
        Some(h) if VIDEO_HOSTS.iter().any(|v| host_matches(h, v)) => ItemKind::Video,
        _ => ItemKind::Article,
    }
}

/// First `http(s)://` token in free text, with trailing punctuation removed.
pub fn extract_first_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|tok| tok.starts_with("http://") || tok.starts_with("https://"))
        .map(|tok| tok.trim_end_matches([',', '.', ';', ')', ']', '>', '"', '\'']))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ItemKind;

    #[test]
    fn normalize_cases() {
        let cases: &[(&str, &str)] = &[
            (
                "  https://Example.com/Path?utm_source=x&b=1#frag  ",
                "https://example.com/Path?b=1",
            ),
            ("http://example.com", "http://example.com/"),
            (
                "https://youtu.be/dQw4w9WgXcQ?si=abc",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ",
            ),
            (
                "https://youtu.be/dQw4w9WgXcQ?t=42",
                "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=42",
            ),
            (
                "https://www.youtube.com/shorts/abc123",
                "https://www.youtube.com/watch?v=abc123",
            ),
            (
                "https://m.youtube.com/watch?v=abc123&feature=share&list=PL1",
                "https://www.youtube.com/watch?v=abc123",
            ),
            (
                "https://www.youtube.com/watch?v=abc123&t=10s",
                "https://www.youtube.com/watch?v=abc123&t=10s",
            ),
            (
                "https://news.site/a?fbclid=1&gclid=2&ref=tw&keep=yes",
                "https://news.site/a?keep=yes",
            ),
            (
                "https://x.com/a/status/1?s=20&t=zzz",
                "https://x.com/a/status/1",
            ),
            ("https://example.com/a/?x=1", "https://example.com/a/?x=1"),
        ];
        for (input, want) in cases {
            let got = normalize_url(input).unwrap_or_else(|e| panic!("{input}: {e}"));
            assert_eq!(got.as_str(), *want, "input {input}");
        }
    }

    #[test]
    fn normalize_rejects_non_http() {
        for bad in [
            "chrome://extensions",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "not a url",
            "",
        ] {
            assert!(
                matches!(normalize_url(bad), Err(crate::StowError::InvalidUrl { .. })),
                "{bad}"
            );
        }
    }

    #[test]
    fn classify_cases() {
        let v = |u: &str| classify_kind(&url::Url::parse(u).unwrap());
        assert_eq!(v("https://www.youtube.com/watch?v=1"), ItemKind::Video);
        assert_eq!(v("https://youtu.be/1"), ItemKind::Video);
        assert_eq!(v("https://vimeo.com/123"), ItemKind::Video);
        assert_eq!(v("https://www.loom.com/share/abc"), ItemKind::Video);
        assert_eq!(v("https://www.twitch.tv/videos/1"), ItemKind::Video);
        assert_eq!(v("https://example.com/post"), ItemKind::Article);
    }

    #[test]
    fn extract_first_url_from_sentence() {
        assert_eq!(
            extract_first_url("check this https://example.com/a?x=1, cool"),
            Some("https://example.com/a?x=1".to_string())
        );
        assert_eq!(
            extract_first_url("https://a.com/x\nhttps://b.com"),
            Some("https://a.com/x".into())
        );
        assert_eq!(extract_first_url("no links here"), None);
        assert_eq!(extract_first_url("chrome://settings"), None);
    }
}
