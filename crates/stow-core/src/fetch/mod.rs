//! Network fetching: page metadata and readable snapshots.

pub mod metadata;
pub mod net;
pub mod snapshot;

use std::time::Duration;

/// Desktop Safari UA plus a `Stow/0.1` suffix; some sites serve bots a stripped page.
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15 Stow/0.1";

fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(15))
}

/// Shared HTTP client for user-supplied URLs: desktop UA, 15 s timeout, and SSRF guards.
/// Names resolving only to non-public addresses fail in [`net::SafeResolver`], every
/// connection's peer must be [public](net::is_public_ip) (this catches literal-IP URLs),
/// redirects are re-checked per hop and capped at 10, and system proxies are ignored
/// (a proxy would resolve names itself, around the guards). Built once at startup
/// (`JobQueue::start`, `Stow::fetch_item`) so a TLS setup failure is an error, not a panic.
pub fn client() -> crate::Result<reqwest::Client> {
    Ok(builder()
        .no_proxy()
        .dns_resolver(net::SafeResolver)
        .connector_layer(net::PublicPeerLayer)
        .redirect(net::redirect_policy())
        .build()?)
}

/// [`client`] without the SSRF guards: local and private addresses are fetched. For
/// tests (wiremock listens on 127.0.0.1) and for users who enable `allow_local_urls`.
pub fn client_allow_local() -> crate::Result<reqwest::Client> {
    Ok(builder()
        .redirect(reqwest::redirect::Policy::limited(net::MAX_REDIRECTS))
        .build()?)
}

/// [`client`] or, when `allow_local` is set, [`client_allow_local`].
pub fn client_for(allow_local: bool) -> crate::Result<reqwest::Client> {
    if allow_local {
        client_allow_local()
    } else {
        client()
    }
}

/// Stack for HTML parsing threads; readability and HTML → Markdown recurse per element.
const PARSER_STACK_BYTES: usize = 64 * 1024 * 1024;

/// Run CPU-bound HTML parsing on its own 64 MiB-stack thread and await the result,
/// keeping it off tokio's workers (small stacks, and they must not block).
pub(crate) async fn on_parser_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> crate::Result<T> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("stow-parser".into())
        .stack_size(PARSER_STACK_BYTES)
        .spawn(move || {
            // The receiver is gone only when the awaiting job was cancelled.
            if tx.send(f()).is_err() {
                tracing::debug!("parser result dropped: caller went away");
            }
        })
        .map_err(|e| crate::StowError::Internal(format!("spawn parser thread: {e}")))?;
    rx.await
        .map_err(|_| crate::StowError::Extract("parser thread panicked".into()))
}

/// Largest page body `fetch_snapshot` and `fetch_metadata` read.
pub const MAX_PAGE_BYTES: u64 = 10 * 1024 * 1024;

/// Read the body, refusing up front when `Content-Length` exceeds `limit` and otherwise
/// abandoning the stream the moment the running total does; at most `limit` bytes are
/// ever buffered.
pub(crate) async fn read_capped(mut resp: reqwest::Response, limit: u64) -> crate::Result<Vec<u8>> {
    let too_large = || crate::StowError::TooLarge { limit_bytes: limit };
    if resp.content_length().is_some_and(|n| n > limit) {
        return Err(too_large());
    }
    let mut bytes: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if (bytes.len() + chunk.len()) as u64 > limit {
            return Err(too_large());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// [`read_capped`] decoded with the `Content-Type` charset (UTF-8 when absent or
/// unknown), replacing malformed sequences like `Response::text` does.
pub(crate) async fn read_text_capped(resp: reqwest::Response, limit: u64) -> crate::Result<String> {
    let encoding = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|ct| {
            ct.split(';')
                .filter_map(|p| p.trim().split_once('='))
                .find(|(k, _)| k.trim().eq_ignore_ascii_case("charset"))
                .map(|(_, v)| v.trim().trim_matches('"').to_string())
        })
        .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    let bytes = read_capped(resp, limit).await?;
    let (text, _, _) = encoding.decode(&bytes);
    Ok(text.into_owned())
}

fn is_html(resp: &reqwest::Response) -> bool {
    resp.headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("text/html") || ct.contains("application/xhtml"))
}

#[cfg(test)]
pub(crate) mod test_support {
    /// Minimal HTTP/1.1 server that writes `head` then `body_chunks` and then stalls
    /// without closing, so a client that reads to the end hangs until its timeout.
    /// Only a client that stops early can finish quickly.
    pub(crate) async fn stalling_server(head: &'static str, body_chunks: Vec<Vec<u8>>) -> String {
        use tokio::io::AsyncWriteExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut req = vec![0u8; 4096];
            let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut req).await;
            let _ = sock.write_all(head.as_bytes()).await;
            for c in body_chunks {
                if sock.write_all(&c).await.is_err() {
                    return;
                }
            }
            // Never send the terminating chunk / remaining bytes; hold the socket open.
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        });
        format!("http://{addr}/big.png")
    }

    /// HTTP/1.1 chunked encoding of `total` bytes, in 64 KiB chunks, without the
    /// terminating zero-length chunk.
    pub(crate) fn chunked(total: usize) -> Vec<Vec<u8>> {
        let chunk = vec![0xABu8; 64 * 1024];
        let mut chunks = Vec::new();
        let mut sent = 0;
        while sent < total {
            let n = chunk.len().min(total - sent);
            chunks.push(format!("{n:x}\r\n").into_bytes());
            chunks.push(chunk[..n].to_vec());
            chunks.push(b"\r\n".to_vec());
            sent += n;
        }
        chunks
    }
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn client_refuses_private_literal_ip() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("hi"))
            .mount(&server)
            .await;
        let url = format!("{}/x", server.uri());
        assert!(url.contains("127.0.0.1"), "{url}");
        let refused = super::client().unwrap().get(&url).send().await;
        assert!(refused.is_err(), "safe client reached {url}");
        let ok = super::client_allow_local().unwrap().get(&url).send().await;
        assert_eq!(ok.unwrap().status(), 200);
    }

    #[tokio::test]
    async fn fetches_time_out_instead_of_hanging() {
        // Header-only servers that never finish the body: with a 300 ms client timeout
        // both fetchers must return `Http` well within a second, not hang on the body.
        use super::test_support::stalling_server;
        use crate::StowError;
        let client = super::builder()
            .timeout(std::time::Duration::from_millis(300))
            .build()
            .unwrap();
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: 4096\r\n\r\n";
        for (what, url) in [
            ("metadata (no body)", stalling_server(head, vec![]).await),
            (
                "metadata (partial body)",
                stalling_server(head, vec![b"<html><head><title>x".to_vec()]).await,
            ),
            ("snapshot (no body)", stalling_server(head, vec![]).await),
            (
                "snapshot (partial body)",
                stalling_server(head, vec![b"<html><body><p>".to_vec()]).await,
            ),
        ] {
            let url = url::Url::parse(&url).unwrap();
            let t = std::time::Instant::now();
            let err = if what.starts_with("metadata") {
                super::metadata::fetch_metadata(&client, &url)
                    .await
                    .map(|_| ())
                    .unwrap_err()
            } else {
                super::snapshot::fetch_snapshot(&client, &url)
                    .await
                    .map(|_| ())
                    .unwrap_err()
            };
            let took = t.elapsed();
            assert!(
                took < std::time::Duration::from_secs(1),
                "{what}: took {took:?}"
            );
            assert!(matches!(err, StowError::Http(_)), "{what}: {err:?}");
            assert_eq!(err.code(), "http", "{what}");
        }
        // A server that never even answers the request line is caught the same way.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (_sock, _) = listener.accept().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        });
        let url = url::Url::parse(&format!("http://{addr}/silent")).unwrap();
        let t = std::time::Instant::now();
        let err = super::metadata::fetch_metadata(&client, &url)
            .await
            .unwrap_err();
        assert!(t.elapsed() < std::time::Duration::from_secs(1));
        assert!(matches!(err, StowError::Http(_)), "{err:?}");
    }

    #[test]
    fn redirect_hops_to_local_hosts_are_refused() {
        use super::net::redirect_allowed;
        let u = |s: &str| url::Url::parse(s).unwrap();
        assert!(redirect_allowed(&u("https://example.com/a"), 0));
        assert!(!redirect_allowed(&u("http://127.0.0.1/a"), 0));
        assert!(!redirect_allowed(&u("http://localhost/a"), 0));
        assert!(!redirect_allowed(&u("http://[fd00::1]/a"), 0));
        assert!(!redirect_allowed(&u("ftp://example.com/a"), 0));
        assert!(redirect_allowed(&u("https://example.com/a"), 9));
        assert!(
            !redirect_allowed(&u("https://example.com/a"), 10),
            "hop limit"
        );
    }
}
