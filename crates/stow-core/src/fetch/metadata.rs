//! Page metadata: `<title>`, meta description, `OpenGraph`, favicon; `YouTube` via oEmbed.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use scraper::{Html, Selector};
use url::Url;

use crate::error::IoContext;
use crate::fetch::{is_html, read_capped, read_text_capped, MAX_PAGE_BYTES};
use crate::{Result, StowError};

/// What a page says about itself. Every field is optional.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Metadata {
    /// `og:title`, else `<title>`.
    pub title: Option<String>,
    /// `og:description`, else `meta[name=description]`.
    pub description: Option<String>,
    /// `og:site_name`, else the host.
    pub site_name: Option<String>,
    /// `link[rel~=icon]`, else `/favicon.ico`.
    pub favicon_url: Option<Url>,
    /// `og:image`, else `twitter:image`.
    pub image_url: Option<Url>,
}

/// Parsed once; the selector strings are literals, so parsing cannot fail at runtime.
fn sel(s: &'static str) -> Selector {
    Selector::parse(s).unwrap_or_else(|e| panic!("invalid selector {s:?}: {e}"))
}

static OG_TITLE: LazyLock<Selector> = LazyLock::new(|| sel(r#"meta[property="og:title"]"#));
static TITLE: LazyLock<Selector> = LazyLock::new(|| sel("title"));
static OG_DESCRIPTION: LazyLock<Selector> =
    LazyLock::new(|| sel(r#"meta[property="og:description"]"#));
static META_DESCRIPTION: LazyLock<Selector> = LazyLock::new(|| sel(r#"meta[name="description"]"#));
static OG_SITE_NAME: LazyLock<Selector> = LazyLock::new(|| sel(r#"meta[property="og:site_name"]"#));
static ICON: LazyLock<Selector> = LazyLock::new(|| sel(r#"link[rel~="icon"]"#));
static OG_IMAGE: LazyLock<Selector> = LazyLock::new(|| sel(r#"meta[property="og:image"]"#));
static TWITTER_IMAGE: LazyLock<Selector> = LazyLock::new(|| sel(r#"meta[name="twitter:image"]"#));

fn attr_of(doc: &Html, selector: &Selector, attr: &str) -> Option<String> {
    doc.select(selector)
        .filter_map(|e| e.value().attr(attr))
        .map(str::trim)
        .find(|v| !v.is_empty())
        .map(str::to_string)
}

fn clean(s: Option<String>) -> Option<String> {
    s.map(|v| v.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|v| !v.is_empty())
}

/// Parse `<head>` metadata. `OpenGraph` wins over plain tags; `site_name` falls back to the host.
pub fn parse_metadata(html: &str, base: &Url) -> Metadata {
    let doc = Html::parse_document(html);
    let title = clean(attr_of(&doc, &OG_TITLE, "content").or_else(|| {
        doc.select(&TITLE)
            .next()
            .map(|t| t.text().collect::<String>())
    }));
    let description = clean(
        attr_of(&doc, &OG_DESCRIPTION, "content")
            .or_else(|| attr_of(&doc, &META_DESCRIPTION, "content")),
    );
    let site_name = clean(attr_of(&doc, &OG_SITE_NAME, "content"))
        .or_else(|| base.host_str().map(str::to_string));
    let favicon_url = attr_of(&doc, &ICON, "href")
        .and_then(|h| base.join(&h).ok())
        .or_else(|| base.join("/favicon.ico").ok());
    let image_url = attr_of(&doc, &OG_IMAGE, "content")
        .or_else(|| attr_of(&doc, &TWITTER_IMAGE, "content"))
        .and_then(|h| base.join(&h).ok());
    Metadata {
        title,
        description,
        site_name,
        favicon_url,
        image_url,
    }
}

/// Parse a `YouTube` (or any oEmbed) JSON response.
pub fn parse_oembed(json: &str, base: &Url) -> Result<Metadata> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    let get = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let provider = get("provider_name").unwrap_or_else(|| "YouTube".into());
    let site_name = Some(match get("author_name") {
        Some(a) => format!("{provider} · {a}"),
        None => provider,
    });
    Ok(Metadata {
        title: clean(get("title")),
        description: None,
        site_name,
        favicon_url: base.join("/favicon.ico").ok(),
        image_url: get("thumbnail_url").and_then(|u| Url::parse(&u).ok()),
    })
}

fn is_youtube(url: &Url) -> bool {
    matches!(url.host_str(), Some(h) if h == "youtube.com" || h.ends_with(".youtube.com") || h == "youtu.be")
}

/// Fetch metadata for a URL. Non-HTML responses yield only `site_name`. Bodies over
/// [`MAX_PAGE_BYTES`] → `TooLarge`; pages nested deeper than
/// [`snapshot::MAX_NESTING`](crate::fetch::snapshot::MAX_NESTING) → `Extract`.
pub async fn fetch_metadata(client: &reqwest::Client, url: &Url) -> Result<Metadata> {
    if is_youtube(url) {
        let oembed = Url::parse_with_params(
            "https://www.youtube.com/oembed",
            &[("url", url.as_str()), ("format", "json")],
        )
        .map_err(|e| StowError::InvalidUrl {
            input: url.to_string(),
            reason: e.to_string(),
        })?;
        let resp = client.get(oembed).send().await?.error_for_status()?;
        let body = read_text_capped(resp, MAX_PAGE_BYTES).await?;
        return parse_oembed(&body, url);
    }
    let resp = client.get(url.clone()).send().await?.error_for_status()?;
    if !is_html(&resp) {
        return Ok(Metadata {
            site_name: url.host_str().map(str::to_string),
            ..Default::default()
        });
    }
    let final_url = resp.url().clone();
    let body = read_text_capped(resp, MAX_PAGE_BYTES).await?;
    crate::fetch::on_parser_thread(move || {
        crate::fetch::snapshot::check_nesting(&body)?;
        Ok(parse_metadata(&body, &final_url))
    })
    .await?
}

/// Largest asset `download_asset` accepts.
const MAX_ASSET_BYTES: u64 = 5 * 1024 * 1024;

/// Download a small image (favicon, preview) into `dest_dir/<stem>.<ext>`. The extension
/// comes only from the `Content-Type`, which must be on the image allowlist (png, jpeg,
/// gif, webp, svg, ico, avif); anything else → `Extract("not an image")`, so a server
/// cannot plant a `.html` or `.app` in the data dir. Max 5 MB: rejected up front when
/// `Content-Length` says so, otherwise abandoned the moment the streamed total exceeds it.
pub async fn download_asset(
    client: &reqwest::Client,
    url: &Url,
    dest_dir: &Path,
    stem: &str,
) -> Result<PathBuf> {
    let resp = client.get(url.clone()).send().await?.error_for_status()?;
    let ext = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| ct.split(';').next())
        .and_then(|m| ext_for_mime(&m.to_ascii_lowercase()))
        .ok_or_else(|| StowError::Extract("not an image".into()))?;
    let bytes = read_capped(resp, MAX_ASSET_BYTES).await?;
    if bytes.is_empty() {
        return Err(StowError::Extract("empty asset".into()));
    }
    std::fs::create_dir_all(dest_dir).io_ctx("create dir", dest_dir)?;
    let path = dest_dir.join(format!("{stem}.{ext}"));
    std::fs::write(&path, &bytes).io_ctx("write", &path)?;
    Ok(path)
}

fn ext_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime.trim() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        "image/avif" => "avif",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StowError;

    fn client() -> reqwest::Client {
        crate::fetch::client_allow_local().unwrap()
    }
    use url::Url;

    const ARTICLE: &str = include_str!("../../tests/fixtures/article.html");
    const BARE: &str = include_str!("../../tests/fixtures/bare_title.html");
    const OEMBED: &str = include_str!("../../tests/fixtures/youtube_oembed.json");

    #[test]
    fn parse_article_prefers_og_tags() {
        let base = Url::parse("https://tabweekly.example/posts/park").unwrap();
        let m = parse_metadata(ARTICLE, &base);
        assert_eq!(m.title.as_deref(), Some("How to Park Tabs"));
        assert_eq!(
            m.description.as_deref(),
            Some("A short guide to saving links with intent.")
        );
        assert_eq!(m.site_name.as_deref(), Some("Tab Weekly"));
        assert_eq!(
            m.image_url.as_ref().map(url::Url::as_str),
            Some("https://tabweekly.example/img/cover.png")
        );
        assert_eq!(
            m.favicon_url.as_ref().map(url::Url::as_str),
            Some("https://tabweekly.example/static/fav.ico")
        );
    }

    #[test]
    fn parse_bare_title_falls_back_to_host_and_favicon_ico() {
        let base = Url::parse("https://plain.example/x").unwrap();
        let m = parse_metadata(BARE, &base);
        assert_eq!(m.title.as_deref(), Some("Just a title"));
        assert_eq!(m.description, None);
        assert_eq!(m.site_name.as_deref(), Some("plain.example"));
        assert_eq!(
            m.favicon_url.as_ref().map(url::Url::as_str),
            Some("https://plain.example/favicon.ico")
        );
        assert_eq!(m.image_url, None);
    }

    #[test]
    fn parse_oembed_youtube() {
        let base = Url::parse("https://www.youtube.com/watch?v=dQw4w9WgXcQ").unwrap();
        let m = parse_oembed(OEMBED, &base).unwrap();
        assert_eq!(m.title.as_deref(), Some("Never Gonna Give You Up"));
        assert_eq!(m.site_name.as_deref(), Some("YouTube · Rick Astley"));
        assert_eq!(
            m.image_url.as_ref().map(url::Url::as_str),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
        assert_eq!(
            m.favicon_url.as_ref().map(url::Url::as_str),
            Some("https://www.youtube.com/favicon.ico")
        );
    }

    #[tokio::test]
    async fn fetch_metadata_non_html_returns_site_only() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/doc.pdf"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(b"%PDF-1.4".to_vec(), "application/pdf"),
            )
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/doc.pdf", server.uri())).unwrap();
        let m = fetch_metadata(&client(), &url).await.unwrap();
        assert_eq!(m.title, None);
        assert_eq!(m.site_name.as_deref(), url.host_str());
    }

    #[tokio::test]
    async fn fetch_metadata_500_is_error() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/x", server.uri())).unwrap();
        assert!(fetch_metadata(&client(), &url).await.is_err());
    }

    #[tokio::test]
    async fn fetch_metadata_html_parses_and_download_asset_writes_file() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(BARE, "text/html; charset=utf-8"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/favicon.ico"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(vec![1u8, 2, 3], "image/png"))
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/post", server.uri())).unwrap();
        let m = fetch_metadata(&client(), &url).await.unwrap();
        assert_eq!(m.title.as_deref(), Some("Just a title"));
        let dir = tempfile::tempdir().unwrap();
        let p = download_asset(
            &client(),
            m.favicon_url.as_ref().unwrap(),
            dir.path(),
            "item1-favicon",
        )
        .await
        .unwrap();
        assert_eq!(
            p.file_name().unwrap().to_str().unwrap(),
            "item1-favicon.png"
        );
        assert_eq!(std::fs::read(&p).unwrap(), vec![1u8, 2, 3]);
    }

    use crate::fetch::test_support::{chunked, stalling_server};

    #[tokio::test]
    async fn download_asset_rejects_oversized_by_content_length() {
        // Content-Length says 6 MB but only a few bytes ever arrive: the rejection must
        // come from the header, before any body is read.
        let url = stalling_server(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 6291456\r\n\r\n",
            vec![vec![0u8; 16]],
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            download_asset(&client(), &Url::parse(&url).unwrap(), dir.path(), "big"),
        )
        .await
        .expect("rejected without reading the body")
        .unwrap_err();
        assert!(
            matches!(err, StowError::TooLarge { limit_bytes } if limit_bytes == 5 * 1024 * 1024),
            "{err}"
        );
        assert_eq!(err.to_string(), "larger than 5 MB");
        assert!(
            std::fs::read_dir(dir.path()).unwrap().next().is_none(),
            "file left on disk"
        );
    }

    #[tokio::test]
    async fn download_asset_rejects_oversized_streaming() {
        // Chunked body with no Content-Length: 5 MB + 1 byte arrive, then the server
        // stalls. The download must abort as soon as the running total exceeds the cap.
        let chunks = chunked(5 * 1024 * 1024 + 1);
        let url = stalling_server(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\n\r\n",
            chunks,
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            download_asset(&client(), &Url::parse(&url).unwrap(), dir.path(), "big"),
        )
        .await
        .expect("aborted as soon as the cap was exceeded")
        .unwrap_err();
        assert!(
            matches!(err, StowError::TooLarge { limit_bytes } if limit_bytes == 5 * 1024 * 1024),
            "{err}"
        );
        assert_eq!(err.to_string(), "larger than 5 MB");
        assert!(
            std::fs::read_dir(dir.path()).unwrap().next().is_none(),
            "file left on disk"
        );
    }

    #[tokio::test]
    async fn fetch_metadata_rejects_oversized_body() {
        let url = stalling_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\n\r\n",
            chunked(10 * 1024 * 1024 + 1),
        )
        .await;
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            fetch_metadata(&client(), &Url::parse(&url).unwrap()),
        )
        .await
        .expect("aborted at the cap")
        .unwrap_err();
        assert!(
            matches!(err, StowError::TooLarge { limit_bytes } if limit_bytes == 10 * 1024 * 1024),
            "{err}"
        );
    }

    #[tokio::test]
    async fn download_asset_rejects_non_image_content_type() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/favicon.ico"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw("<script>x</script>", "text/html"),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/evil.app"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8, 2, 3]))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        for p in ["/favicon.ico", "/evil.app"] {
            let url = Url::parse(&format!("{}{p}", server.uri())).unwrap();
            let err = download_asset(&client(), &url, dir.path(), "x")
                .await
                .expect_err(p);
            assert!(
                matches!(&err, StowError::Extract(m) if m == "not an image"),
                "{p}: {err:?}"
            );
        }
        assert!(
            !dir.path().exists() || std::fs::read_dir(dir.path()).unwrap().next().is_none(),
            "nothing written"
        );
    }
}
