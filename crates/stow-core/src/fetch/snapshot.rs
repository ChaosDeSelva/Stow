//! Readable snapshots: readability extraction → HTML + Markdown files + search text.

use std::path::{Path, PathBuf};

use dom_smoothie::{Config, Readability};
use url::Url;

use crate::error::IoContext;
use crate::fetch::{is_html, read_text_capped, MAX_PAGE_BYTES};
use crate::{Result, StowError};

/// One extracted page.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    /// Article title, when readability found one.
    pub title: Option<String>,
    /// Cleaned article HTML.
    pub html: String,
    /// The article as Markdown (with a `# title` heading).
    pub markdown: String,
    /// Plain text for the search index.
    pub text: String,
}

/// Deepest element nesting [`extract`] and metadata parsing accept. Readability and
/// HTML → Markdown recurse once per level; real pages stay far below this.
pub const MAX_NESTING: usize = 512;

/// Tags that never add a level: void elements, and elements whose end tag is optional
/// (the parser closes them implicitly, so counting them would flag flat pages).
const FLAT_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr", "p", "li", "dt", "dd", "option", "optgroup", "tr", "td", "th", "thead",
    "tbody", "tfoot", "colgroup", "caption", "rb", "rp", "rt", "rtc", "html", "head", "body",
];

/// Elements whose content is text, so a `<div>` inside them is not a tag.
const RAW_TEXT_TAGS: &[&str] = &["script", "style", "textarea", "title", "xmp"];

/// Cheap upper bound on element nesting, without building a DOM. Counts opening tags
/// minus matching closing tags, skipping comments, raw-text bodies, [`FLAT_TAGS`], and
/// `/>` inside `<svg>`/`<math>` (the only place it self-closes). Approximate by design;
/// it only has to bound what the recursive parsers see.
pub fn check_nesting(html: &str) -> Result<()> {
    let lower = html.to_ascii_lowercase();
    let b = lower.as_bytes();
    let find = |from: usize, pat: &[u8]| -> Option<usize> {
        b.get(from..)?
            .windows(pat.len())
            .position(|w| w == pat)
            .map(|p| p + from)
    };
    let mut depth = 0usize;
    // Depth at which the current <svg>/<math> subtree started.
    let mut foreign_from: Option<usize> = None;
    let mut i = 0;
    while let Some(lt) = find(i, b"<") {
        i = lt + 1;
        if b[i..].starts_with(b"!--") {
            i = find(i + 3, b"-->").map_or(b.len(), |e| e + 3);
            continue;
        }
        let closing = b.get(i) == Some(&b'/');
        if closing {
            i += 1;
        }
        let name_len = b[i..]
            .iter()
            .take_while(|c| c.is_ascii_alphanumeric() || **c == b'-' || **c == b':')
            .count();
        if name_len == 0 || !b[i].is_ascii_alphabetic() {
            continue;
        }
        let name = &lower[i..i + name_len];
        let Some(gt) = find(i + name_len, b">") else {
            break;
        };
        let self_closing = gt > 0 && b[gt - 1] == b'/';
        i = gt + 1;
        if FLAT_TAGS.contains(&name) {
            continue;
        }
        if closing {
            depth = depth.saturating_sub(1);
            if foreign_from.is_some_and(|f| depth <= f) {
                foreign_from = None;
            }
        } else if RAW_TEXT_TAGS.contains(&name) {
            let end = format!("</{name}");
            i = find(i, end.as_bytes()).unwrap_or(b.len());
        } else if !(self_closing && foreign_from.is_some()) {
            if foreign_from.is_none() && (name == "svg" || name == "math") {
                foreign_from = Some(depth);
            }
            depth += 1;
            if depth > MAX_NESTING {
                return Err(StowError::Extract("nesting too deep".into()));
            }
        }
    }
    Ok(())
}

/// [`extract`] on a dedicated thread with a large stack, so a pathological page fails
/// one job instead of overflowing a tokio worker's stack and aborting the process.
pub async fn extract_blocking(html: String, base: Url) -> Result<Snapshot> {
    crate::fetch::on_parser_thread(move || extract(&html, &base)).await?
}

/// Readability-style extraction, then HTML → Markdown, then a plain-text copy for search.
/// Pages nested deeper than [`MAX_NESTING`] → `Extract("nesting too deep")`. From async
/// code use [`extract_blocking`].
pub fn extract(html: &str, base: &Url) -> Result<Snapshot> {
    check_nesting(html)?;
    let cfg = Config {
        ..Default::default()
    };
    let mut r = Readability::new(html, Some(base.as_str()), Some(cfg))
        .map_err(|e| StowError::Extract(format!("readability: {e}")))?;
    let article = r
        .parse()
        .map_err(|e| StowError::Extract(format!("readability: {e}")))?;
    let content = article.content.to_string();
    let title = Some(article.title.trim().to_string()).filter(|t| !t.is_empty());
    let body_md = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec![
            "script", "style", "nav", "aside", "footer", "iframe", "noscript",
        ])
        .build()
        .convert(&content)
        .map_err(|e| StowError::Extract(format!("html→md: {e}")))?;
    let markdown = match &title {
        Some(t) if !body_md.trim_start().starts_with('#') => format!("# {t}\n\n{}", body_md.trim()),
        _ => body_md.trim().to_string(),
    };
    let text = markdown_to_text(&markdown);
    Ok(Snapshot {
        title,
        html: content,
        markdown,
        text,
    })
}

/// Strip common Markdown syntax so FTS indexes words, not punctuation.
pub fn markdown_to_text(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    for line in md.lines() {
        let l = line.trim_start_matches(['#', '>', ' ']);
        let l = l
            .strip_prefix("- ")
            .or_else(|| l.strip_prefix("* "))
            .unwrap_or(l);
        let mut cleaned = String::with_capacity(l.len());
        let mut chars = l.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '*' | '_' | '`' | '[' | ']' => {}
                '(' if cleaned.ends_with(']') => {}
                '(' => {
                    // drop link targets: "(http...)" immediately after link text
                    let rest: String = chars.clone().take(4).collect();
                    if rest.starts_with("http") {
                        for n in chars.by_ref() {
                            if n == ')' {
                                break;
                            }
                        }
                    } else {
                        cleaned.push(c);
                    }
                }
                _ => cleaned.push(c),
            }
        }
        if !cleaned.trim().is_empty() {
            out.push_str(cleaned.trim());
            out.push('\n');
        }
    }
    out
}

/// GET `url` and extract it on a parser thread. Non-HTML responses →
/// [`StowError::NotHtml`]; bodies over [`MAX_PAGE_BYTES`] → [`StowError::TooLarge`].
pub async fn fetch_snapshot(client: &reqwest::Client, url: &Url) -> Result<Snapshot> {
    let resp = client.get(url.clone()).send().await?.error_for_status()?;
    if !is_html(&resp) {
        return Err(StowError::NotHtml);
    }
    let final_url = resp.url().clone();
    let body = read_text_capped(resp, MAX_PAGE_BYTES).await?;
    extract_blocking(body, final_url).await
}

/// `(snapshots/<id>.html, snapshots/<id>.md)`.
pub fn snapshot_paths(data_dir: &Path, item_id: &str) -> (PathBuf, PathBuf) {
    let dir = data_dir.join("snapshots");
    (
        dir.join(format!("{item_id}.html")),
        dir.join(format!("{item_id}.md")),
    )
}

/// Write both snapshot files; returns their paths.
pub fn write_snapshot(
    data_dir: &Path,
    item_id: &str,
    snap: &Snapshot,
) -> Result<(PathBuf, PathBuf)> {
    let (html, md) = snapshot_paths(data_dir, item_id);
    let dir = data_dir.join("snapshots");
    std::fs::create_dir_all(&dir).io_ctx("create dir", &dir)?;
    std::fs::write(&html, &snap.html).io_ctx("write", &html)?;
    std::fs::write(&md, &snap.markdown).io_ctx("write", &md)?;
    Ok((html, md))
}

/// The Markdown snapshot, `None` when it was never fetched.
pub fn read_snapshot_md(data_dir: &Path, item_id: &str) -> Result<Option<String>> {
    let (_, md) = snapshot_paths(data_dir, item_id);
    match std::fs::read_to_string(&md) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        r => r.map(Some).io_ctx("read", &md),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    const READABLE: &str = include_str!("../../tests/fixtures/readable.html");

    #[test]
    fn extract_readable_drops_nav_and_keeps_paragraphs() {
        let base = Url::parse("https://example.com/article").unwrap();
        let s = extract(READABLE, &base).unwrap();
        assert_eq!(s.title.as_deref(), Some("Readable Article"));
        assert!(
            !s.text.contains("Subscribe to our newsletter"),
            "aside should be dropped: {}",
            s.text
        );
        assert!(!s.text.contains("Contact"), "nav should be dropped");
        assert!(s.text.contains("zebra crossing"));
        assert!(s.text.contains("first paragraph") && s.text.contains("second paragraph"));
    }

    #[test]
    fn extract_markdown_has_heading_and_no_tags() {
        let base = Url::parse("https://example.com/article").unwrap();
        let s = extract(READABLE, &base).unwrap();
        assert!(
            s.markdown.contains("# Readable Article") || s.markdown.contains("Readable Article\n="),
            "{}",
            s.markdown
        );
        assert!(!s.markdown.contains("<p>"));
        assert!(
            !s.text.contains('#'),
            "text should strip markdown syntax: {}",
            s.text
        );
    }

    #[test]
    fn extract_rejects_deeply_nested_html() {
        let base = Url::parse("https://example.com/deep").unwrap();
        let html = format!(
            "<html><body>{}x{}</body></html>",
            "<div>".repeat(100_000),
            "</div>".repeat(100_000)
        );
        let err = extract(&html, &base).expect_err("must refuse, not crash");
        assert!(
            matches!(&err, StowError::Extract(m) if m == "nesting too deep"),
            "{err:?}"
        );
    }

    #[test]
    fn nesting_check_allows_flat_pages_with_void_and_optional_end_tags() {
        // 2000 unclosed <p>/<li>, void tags, comments and scripts full of "<div>" must
        // not count as depth.
        let mut html =
            String::from("<html><head><meta charset=utf-8><link rel=icon href=x></head><body><ul>");
        for _ in 0..2000 {
            html.push_str("<li><p>para<br><img src=a.png><input/><!-- <div><div> -->");
        }
        html.push_str("<script>var s = '<div>'.repeat(9999);</script></ul></body></html>");
        assert!(check_nesting(&html).is_ok());
        let ok_deep = format!("{}{}", "<div>".repeat(500), "</div>".repeat(500));
        assert!(check_nesting(&ok_deep).is_ok());
        let too_deep = "<section>".repeat(513);
        assert!(check_nesting(&too_deep).is_err());
        // Stray closing tags must not hide real depth.
        let hidden = "<div></p></br>".repeat(600);
        assert!(check_nesting(&hidden).is_err());
    }

    #[tokio::test]
    async fn extract_blocking_handles_allowed_depth_off_thread() {
        let base = Url::parse("https://example.com/deep").unwrap();
        let html = format!(
            "<html><head><title>Deep</title></head><body>{}<p>The zebra crossing paragraph is long enough to be readable content for the extractor.</p>{}</body></html>",
            "<div>".repeat(500),
            "</div>".repeat(500)
        );
        let s = extract_blocking(html, base).await.unwrap();
        assert!(s.text.contains("zebra"), "{}", s.text);
    }

    #[test]
    fn write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("snapshots")).unwrap();
        let snap = Snapshot {
            title: Some("T".into()),
            html: "<p>x</p>".into(),
            markdown: "x".into(),
            text: "x".into(),
        };
        let (html, md) = write_snapshot(dir.path(), "01ABC", &snap).unwrap();
        assert!(html.ends_with("snapshots/01ABC.html") && md.ends_with("snapshots/01ABC.md"));
        assert_eq!(
            read_snapshot_md(dir.path(), "01ABC").unwrap().as_deref(),
            Some("x")
        );
        assert_eq!(read_snapshot_md(dir.path(), "nope").unwrap(), None);
    }

    #[tokio::test]
    async fn fetch_snapshot_rejects_non_html() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(b"%PDF".to_vec(), "application/pdf"),
            )
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/f.pdf", server.uri())).unwrap();
        let err = fetch_snapshot(&crate::fetch::client_allow_local().unwrap(), &url)
            .await
            .expect_err("should fail");
        assert!(matches!(err, StowError::NotHtml), "{err}");
        assert_eq!(err.code(), "not_html");
    }

    #[tokio::test]
    async fn fetch_snapshot_rejects_oversized_body() {
        use crate::fetch::test_support::{chunked, stalling_server};
        // Content-Length over the cap: rejected before any body is read.
        let url = stalling_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 11534336\r\n\r\n",
            vec![vec![b'<'; 16]],
        )
        .await;
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            fetch_snapshot(
                &crate::fetch::client_allow_local().unwrap(),
                &Url::parse(&url).unwrap(),
            ),
        )
        .await
        .expect("rejected from the header")
        .unwrap_err();
        assert!(
            matches!(err, StowError::TooLarge { limit_bytes } if limit_bytes == 10 * 1024 * 1024),
            "{err}"
        );
        // No Content-Length: aborted as soon as the streamed total passes the cap.
        let url = stalling_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nTransfer-Encoding: chunked\r\n\r\n",
            chunked(10 * 1024 * 1024 + 1),
        )
        .await;
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            fetch_snapshot(
                &crate::fetch::client_allow_local().unwrap(),
                &Url::parse(&url).unwrap(),
            ),
        )
        .await
        .expect("aborted at the cap")
        .unwrap_err();
        assert!(matches!(err, StowError::TooLarge { .. }), "{err}");
    }

    #[tokio::test]
    async fn fetch_snapshot_decodes_declared_charset() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        // "café" in ISO-8859-1.
        let mut body =
            b"<html><head><title>Caf\xe9 guide</title></head><body><article><p>".to_vec();
        body.extend_from_slice(b"A long paragraph about the caf\xe9 on the corner, long enough for readability to keep it as content.</p></article></body></html>");
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(200).set_body_raw(body, "text/html; charset=iso-8859-1"),
            )
            .mount(&server)
            .await;
        let url = Url::parse(&format!("{}/c", server.uri())).unwrap();
        let s = fetch_snapshot(&crate::fetch::client_allow_local().unwrap(), &url)
            .await
            .unwrap();
        assert!(s.text.contains("café"), "{}", s.text);
    }
}
