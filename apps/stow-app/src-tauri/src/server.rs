//! Localhost HTTP endpoint for the Stow Chrome extension.
//!
//! Bound to `127.0.0.1:<port>` (saved port, then 47823, then any); the port is written to
//! the `extension_port` setting and the bearer token lives in `extension_token`. The
//! extension (`extension/background.js`) POSTs `/capture`, GETs `/buckets` with
//! `authorization: Bearer <token>`, and can ask for the token with `POST /pair`, which
//! waits for the user to click Allow in the app.
//!
//! Hardening: only `Host: 127.0.0.1:<port>` / `localhost:<port>` is served (DNS
//! rebinding), the token is re-read from settings on every request (rotation is
//! immediate) and compared in constant time, and bodies, URLs and titles are capped.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use stow_core::error::IoContext;
use stow_core::jobs::JobQueue;
use stow_core::repo::{buckets, settings};
use stow_core::{CaptureRequest, Db, Result, Stow, StowError};
use subtle::ConstantTimeEq;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::events::PairRequest;
use crate::pairing::PairingState;

const TOKEN_KEY: &str = "extension_token";
const PORT_KEY: &str = "extension_port";
/// When set (the 32-char id shown on `chrome://extensions`), only that extension's origin
/// passes CORS. Unset = any `chrome-extension://` origin, since the id of an unpacked
/// extension is not known in advance.
pub const EXTENSION_ID_KEY: &str = "extension_id";

/// JSON bodies larger than this are refused with 413.
const BODY_LIMIT: usize = 16 * 1024;
const MAX_URL: usize = 8 * 1024;
const MAX_TITLE: usize = 1024;
const MAX_PAIR_NAME: usize = 100;
/// How long `POST /pair` waits for Allow/Deny.
pub const PAIR_TIMEOUT: Duration = Duration::from_secs(60);

/// Called when an extension asks to pair; the app shows the main window and emits
/// `pair-request`.
pub type PairNotifier = Arc<dyn Fn(PairRequest) + Send + Sync>;

pub struct ServerInfo {
    pub port: u16,
    #[allow(dead_code)] // surfaced through settings, kept for callers/tests
    pub token: String,
}

/// Everything the router needs.
#[derive(Clone)]
pub struct ServerCtx {
    pub db: Arc<Db>,
    pub jobs: JobQueue,
    /// The bound port; requests must name it in `Host`.
    pub port: u16,
    pub pairing: Arc<PairingState>,
    pub on_pair: PairNotifier,
    pub pair_timeout: Duration,
}

/// Generates/loads the token, binds, stores the chosen port in settings, spawns the axum
/// server on the current tokio runtime and returns immediately.
pub async fn start(
    db: Arc<Db>,
    jobs: JobQueue,
    pairing: Arc<PairingState>,
    on_pair: PairNotifier,
) -> Result<ServerInfo> {
    let token = load_or_create_token(&db)?;
    let listener = bind_preferred(&db).await?;
    let port = listener.local_addr().io_ctx("bind", "127.0.0.1")?.port();
    settings::set(&db, PORT_KEY, &port.to_string())?;
    let app = router(ServerCtx {
        db,
        jobs,
        port,
        pairing,
        on_pair,
        pair_timeout: PAIR_TIMEOUT,
    });
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("extension server stopped: {e}");
        }
    });
    tracing::info!(port, "extension server listening");
    Ok(ServerInfo { port, token })
}

/// Port the extension was last told about, if any.
const DEFAULT_PORT: u16 = 47823;

/// Try the saved port, then the default, then anything free, so the extension's stored
/// port keeps working across restarts whenever possible.
async fn bind_preferred(db: &Db) -> Result<tokio::net::TcpListener> {
    let saved: Option<u16> = settings::get(db, PORT_KEY)?
        .and_then(|p| p.parse().ok())
        .filter(|p| *p != 0);
    for port in saved.into_iter().chain([DEFAULT_PORT]) {
        if let Ok(l) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
            return Ok(l);
        }
    }
    tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .io_ctx("bind", "127.0.0.1:0")
}

/// Build the router (used by `start` and by tests).
pub fn router(ctx: ServerCtx) -> Router {
    let protected = Router::new()
        .route("/buckets", get(list_buckets))
        .route("/capture", post(capture))
        .route_layer(middleware::from_fn_with_state(ctx.clone(), require_token));
    let cors_db = ctx.db.clone();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            // A quick indexed read; only requests that carry an Origin get here.
            let pinned = settings::get(&cors_db, EXTENSION_ID_KEY).ok().flatten();
            origin_allowed(origin.as_bytes(), pinned.as_deref())
        }))
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);
    Router::new()
        .route("/health", get(health))
        .route("/pair", post(pair))
        .merge(protected)
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(middleware::from_fn_with_state(
            ctx.clone(),
            require_local_host,
        ))
        .layer(cors)
        .with_state(ctx)
}

/// `chrome-extension://<id>` where `<id>` is `pinned` when set, anything otherwise.
pub fn origin_allowed(origin: &[u8], pinned: Option<&str>) -> bool {
    let Some(id) = origin.strip_prefix(b"chrome-extension://") else {
        return false;
    };
    match pinned.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => id == p.as_bytes(),
        None => !id.is_empty(),
    }
}

/// `Host` must be exactly `127.0.0.1:<port>` or `localhost:<port>`.
pub fn host_allowed(host: Option<&str>, port: u16) -> bool {
    host.is_some_and(|h| h == format!("127.0.0.1:{port}") || h == format!("localhost:{port}"))
}

/// The token is 32 lowercase hex chars, generated once and kept across restarts.
fn load_or_create_token(db: &Db) -> Result<String> {
    if let Some(t) = settings::get(db, TOKEN_KEY)?.filter(|t| !t.is_empty()) {
        return Ok(t);
    }
    rotate_token(db)
}

/// Replace the token; the old one stops working on the next request.
pub fn rotate_token(db: &Db) -> Result<String> {
    let token = format!("{:032x}", rand::random::<u128>());
    settings::set(db, TOKEN_KEY, &token)?;
    Ok(token)
}

/// `"<port>:<token>"`, what the user pastes into the extension.
pub fn pairing_code(db: &Db) -> Result<String> {
    let token = load_or_create_token(db)?;
    let port = settings::get_or(db, PORT_KEY, "0")?;
    Ok(format!("{port}:{token}"))
}

/// Constant-time token check; an empty stored token never matches.
pub fn token_matches(presented: Option<&str>, stored: &str) -> bool {
    match presented {
        Some(p) if !stored.is_empty() => bool::from(p.as_bytes().ct_eq(stored.as_bytes())),
        _ => false,
    }
}

enum ApiError {
    Status(StatusCode, String),
    Internal(String),
}

impl ApiError {
    fn bad_request(m: impl Into<String>) -> ApiError {
        ApiError::Status(StatusCode::BAD_REQUEST, m.into())
    }
}

impl From<StowError> for ApiError {
    fn from(e: StowError) -> ApiError {
        match e {
            StowError::InvalidUrl { .. } | StowError::InvalidInput { .. } => {
                ApiError::bad_request(e.to_string())
            }
            other => ApiError::Internal(other.to_string()),
        }
    }
}

impl From<JsonRejection> for ApiError {
    fn from(r: JsonRejection) -> ApiError {
        // Keeps 413 for an over-limit body; malformed JSON is 400/415/422.
        ApiError::Status(r.status(), r.body_text())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::Status(s, m) => (s, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

fn status_error(status: StatusCode, msg: &str) -> Response {
    ApiError::Status(status, msg.to_string()).into_response()
}

type ApiResult<T> = std::result::Result<T, ApiError>;

async fn require_local_host(
    State(ctx): State<ServerCtx>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if !host_allowed(host, ctx.port) {
        return status_error(StatusCode::BAD_REQUEST, "bad host");
    }
    next.run(req).await
}

async fn require_token(
    State(ctx): State<ServerCtx>,
    headers: HeaderMap,
    req: Request,
    next: Next,
) -> Response {
    let presented = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .map(str::to_string);
    let stored = match ctx.db.call(|db| settings::get_or(db, TOKEN_KEY, "")).await {
        Ok(t) => t,
        Err(e) => return ApiError::Internal(e.to_string()).into_response(),
    };
    if !token_matches(presented.as_deref(), &stored) {
        return status_error(StatusCode::UNAUTHORIZED, "unauthorized");
    }
    next.run(req).await
}

async fn health() -> Json<Value> {
    Json(json!({ "app": "stow" }))
}

async fn list_buckets(
    State(ctx): State<ServerCtx>,
) -> ApiResult<Json<Vec<stow_core::models::Bucket>>> {
    Ok(Json(ctx.db.call(buckets::list).await?))
}

/// Save the URL; fetch jobs are only queued for a genuinely new item, since a
/// duplicate already has (or is getting) its metadata and snapshot.
async fn capture(
    State(ctx): State<ServerCtx>,
    body: std::result::Result<Json<CaptureRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(mut req) = body?;
    if req.url.len() > MAX_URL {
        return Err(ApiError::bad_request("url too long"));
    }
    if req.title.as_ref().is_some_and(|t| t.len() > MAX_TITLE) {
        return Err(ApiError::bad_request("title too long"));
    }
    let (db, jobs) = (ctx.db.clone(), ctx.jobs.clone());
    let created = tokio::task::spawn_blocking(move || {
        // A default bucket the user has since deleted falls back to the Inbox.
        if let Some(id) = &req.bucket_id {
            if buckets::get(&db, id).is_err() {
                req.bucket_id = None;
            }
        }
        Stow::with_jobs(db, jobs).capture(req)
    })
    .await
    .map_err(|e| ApiError::Internal(e.to_string()))??;
    let item = created.value;
    Ok(Json(
        json!({ "id": item.id, "title": item.title, "created": created.created }),
    ))
}

#[derive(serde::Deserialize)]
struct PairBody {
    #[serde(default)]
    name: String,
}

/// Removes the pending request however the handler ends (answer, timeout, or the
/// extension hanging up), so the one-at-a-time slot is always freed.
struct PendingGuard(Arc<PairingState>, String);

impl Drop for PendingGuard {
    fn drop(&mut self) {
        self.0.cancel(&self.1);
    }
}

/// Ask the user to allow this extension; 200 `{token, port}` on Allow, 403 on Deny or
/// timeout, 429 while another request is waiting.
async fn pair(
    State(ctx): State<ServerCtx>,
    body: std::result::Result<Json<PairBody>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let Json(body) = body?;
    let name = body.name.trim();
    if name.chars().count() > MAX_PAIR_NAME {
        return Err(ApiError::bad_request("name too long"));
    }
    let name = if name.is_empty() {
        "Browser extension".to_string()
    } else {
        name.to_string()
    };
    let Some((id, rx)) = ctx.pairing.begin() else {
        return Err(ApiError::Status(
            StatusCode::TOO_MANY_REQUESTS,
            "another pairing request is waiting".into(),
        ));
    };
    let _guard = PendingGuard(ctx.pairing.clone(), id.clone());
    (ctx.on_pair)(PairRequest { id, name });
    match tokio::time::timeout(ctx.pair_timeout, rx).await {
        Ok(Ok(true)) => {
            let token = ctx.db.call(load_or_create_token).await?;
            Ok(Json(json!({ "token": token, "port": ctx.port })))
        }
        _ => Err(ApiError::Status(
            StatusCode::FORBIDDEN,
            "pairing denied".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::future::IntoFuture;
    use std::time::Duration;
    use stow_core::repo::{buckets, items, settings};

    /// Fresh directory under the system temp dir, removed on drop (`tempfile` is not a
    /// dependency of this crate).
    struct TempDir(std::path::PathBuf);

    impl TempDir {
        fn new() -> TempDir {
            let p = std::env::temp_dir()
                .join(format!("stow-server-test-{:016x}", rand::random::<u64>()));
            std::fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct TestServer {
        _dir: TempDir,
        db: Arc<Db>,
        token: String,
        port: u16,
        base: String,
        http: reqwest::Client,
        pairing: Arc<PairingState>,
        pair_requests: tokio::sync::mpsc::UnboundedReceiver<PairRequest>,
    }

    impl TestServer {
        fn url(&self, path: &str) -> String {
            format!("{}{path}", self.base)
        }

        fn get(&self, path: &str, token: Option<&str>) -> reqwest::RequestBuilder {
            let r = self.http.get(self.url(path));
            match token {
                Some(t) => r.header("authorization", format!("Bearer {t}")),
                None => r,
            }
        }

        fn post(&self, path: &str, token: Option<&str>, body: Value) -> reqwest::RequestBuilder {
            let r = self.http.post(self.url(path)).json(&body);
            match token {
                Some(t) => r.header("authorization", format!("Bearer {t}")),
                None => r,
            }
        }
    }

    /// Bind the router on an ephemeral port the same way `start` does, but with a
    /// known token and a throwaway database.
    async fn serve() -> TestServer {
        serve_with_timeout(Duration::from_secs(5)).await
    }

    async fn serve_with_timeout(pair_timeout: Duration) -> TestServer {
        let dir = TempDir::new();
        let db = Arc::new(Db::open(&dir.0).unwrap());
        let jobs = JobQueue::start(db.clone(), JobQueue::noop_hook()).unwrap();
        let token = "0123456789abcdef0123456789abcdef".to_string();
        settings::set(&db, "extension_token", &token).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, pair_requests) = tokio::sync::mpsc::unbounded_channel();
        let pairing = Arc::new(PairingState::default());
        let ctx = ServerCtx {
            db: db.clone(),
            jobs,
            port,
            pairing: pairing.clone(),
            on_pair: Arc::new(move |r| {
                let _ = tx.send(r);
            }),
            pair_timeout,
        };
        tokio::spawn(axum::serve(listener, router(ctx)).into_future());
        TestServer {
            _dir: dir,
            db,
            token,
            port,
            base: format!("http://127.0.0.1:{port}"),
            http: reqwest::Client::new(),
            pairing,
            pair_requests,
        }
    }

    fn start_args() -> (Arc<PairingState>, PairNotifier) {
        (Arc::new(PairingState::default()), Arc::new(|_| {}))
    }

    /// A URL nothing will answer on, so the enqueued fetch jobs fail fast instead of
    /// reaching the network.
    fn local_url(path: &str) -> String {
        format!("http://127.0.0.1:9/{path}")
    }

    #[tokio::test]
    async fn health_is_open_and_omits_version() {
        let s = serve().await;
        let res = s.get("/health", None).send().await.unwrap();
        assert_eq!(res.status(), 200);
        let body: Value = res.json().await.unwrap();
        assert_eq!(body, json!({"app": "stow"}));
    }

    #[tokio::test]
    async fn capture_requires_token() {
        let s = serve().await;
        let body = json!({"url": local_url("a"), "title": null, "bucket_id": null});
        let res = s.post("/capture", None, body.clone()).send().await.unwrap();
        assert_eq!(res.status(), 401);
        assert_eq!(
            res.json::<Value>().await.unwrap(),
            json!({"error": "unauthorized"})
        );
        let res = s
            .post("/capture", Some("wrong-token"), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 401);
        assert_eq!(
            res.json::<Value>().await.unwrap(),
            json!({"error": "unauthorized"})
        );
        assert!(
            items::list(&s.db, items::ItemFilter::All, true)
                .unwrap()
                .is_empty(),
            "nothing stored"
        );
    }

    #[tokio::test]
    async fn capture_adds_item_and_returns_id() {
        let s = serve().await;
        let b = buckets::create(&s.db, "Read later", None).unwrap();
        let body = json!({"url": local_url("post"), "title": "A post", "bucket_id": b.id});
        let res = s
            .post("/capture", Some(&s.token), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let out: Value = res.json().await.unwrap();
        assert_eq!(out["created"], true);
        assert_eq!(out["title"], "A post");
        let id = out["id"].as_str().expect("id");
        let it = items::get(&s.db, id).unwrap();
        assert_eq!(it.url, local_url("post"));
        assert_eq!(it.bucket_id.as_deref(), Some(b.id.as_str()));
        assert_eq!(it.title.as_deref(), Some("A post"));
    }

    #[tokio::test]
    async fn capture_duplicate_returns_created_false() {
        let s = serve().await;
        let body = json!({"url": local_url("dup")});
        let first: Value = s
            .post("/capture", Some(&s.token), body.clone())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(first["created"], true);
        let res = s
            .post("/capture", Some(&s.token), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        let second: Value = res.json().await.unwrap();
        assert_eq!(second["created"], false);
        assert_eq!(second["id"], first["id"]);
        assert_eq!(
            items::list(&s.db, items::ItemFilter::All, true)
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn capture_rejects_chrome_url_with_400() {
        let s = serve().await;
        let body = json!({"url": "chrome://newtab", "title": "New tab"});
        let res = s
            .post("/capture", Some(&s.token), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400);
        let out: Value = res.json().await.unwrap();
        assert!(
            out["error"]
                .as_str()
                .is_some_and(|e| e.contains("invalid url")),
            "{out}"
        );
        assert_eq!(
            items::list(&s.db, items::ItemFilter::All, true).unwrap(),
            [] as [stow_core::models::Item; 0]
        );
    }

    #[tokio::test]
    async fn start_persists_token_and_port_and_serves() {
        let dir = TempDir::new();
        let db = Arc::new(Db::open(&dir.0).unwrap());
        let jobs = JobQueue::start(db.clone(), JobQueue::noop_hook()).unwrap();
        let (p, n) = start_args();
        let info = start(db.clone(), jobs.clone(), p.clone(), n.clone())
            .await
            .unwrap();
        assert_eq!(info.token.len(), 32);
        assert!(
            info.token
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "{}",
            info.token
        );
        assert_eq!(
            settings::get(&db, "extension_token").unwrap().as_deref(),
            Some(info.token.as_str())
        );
        assert_eq!(
            settings::get(&db, "extension_port").unwrap(),
            Some(info.port.to_string())
        );
        let res = reqwest::get(format!("http://localhost:{}/health", info.port))
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
        // A second start reuses the stored token and records the new port.
        let again = start(db.clone(), jobs, p, n).await.unwrap();
        assert_eq!(again.token, info.token);
        assert_eq!(
            settings::get(&db, "extension_port").unwrap(),
            Some(again.port.to_string())
        );
    }

    #[tokio::test]
    async fn start_prefers_saved_port_and_falls_back_when_taken() {
        let dir = TempDir::new();
        let db = Arc::new(Db::open(&dir.0).unwrap());
        let jobs = JobQueue::start(db.clone(), JobQueue::noop_hook()).unwrap();
        // Pick a free port, save it, and expect start() to bind exactly that one.
        let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let free = probe.local_addr().unwrap().port();
        drop(probe);
        settings::set(&db, "extension_port", &free.to_string()).unwrap();
        let (p, n) = start_args();
        let info = start(db.clone(), jobs.clone(), p.clone(), n.clone())
            .await
            .unwrap();
        assert_eq!(info.port, free, "saved port should be reused");
        // The saved port is now held by the first server; a second start must still succeed.
        let again = start(db.clone(), jobs, p, n).await.unwrap();
        assert_ne!(again.port, free);
        assert_eq!(
            settings::get(&db, "extension_port").unwrap(),
            Some(again.port.to_string())
        );
    }

    #[tokio::test]
    async fn capture_with_missing_bucket_falls_back_to_inbox() {
        let s = serve().await;
        let body = json!({"url": local_url("orphan"), "bucket_id": "01DOESNOTEXIST000000000000"});
        let res = s
            .post("/capture", Some(&s.token), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200, "{}", res.text().await.unwrap());
        let v: Value = res.json().await.unwrap();
        let it = items::get(&s.db, v["id"].as_str().unwrap()).unwrap();
        assert_eq!(it.bucket_id, None);
    }

    #[tokio::test]
    async fn buckets_lists_with_token() {
        let s = serve().await;
        let a = buckets::create(&s.db, "A", Some("📚")).unwrap();
        let b = buckets::create(&s.db, "B", None).unwrap();
        let res = s.get("/buckets", None).send().await.unwrap();
        assert_eq!(res.status(), 401);
        let res = s.get("/buckets", Some(&s.token)).send().await.unwrap();
        assert_eq!(res.status(), 200);
        let got: Vec<stow_core::models::Bucket> = res.json().await.unwrap();
        assert_eq!(got, vec![a, b]);
    }

    #[tokio::test]
    async fn rejects_foreign_host_header() {
        let s = serve().await;
        for bad in [
            "evil.example",
            "evil.example:80",
            "127.0.0.1",
            "127.0.0.1:1",
        ] {
            let res = s
                .get("/health", None)
                .header("host", bad)
                .send()
                .await
                .unwrap();
            assert_eq!(res.status(), 400, "{bad}");
        }
        let res = s
            .get("/health", None)
            .header("host", format!("localhost:{}", s.port))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 200);
    }

    #[test]
    fn host_allowed_cases() {
        assert!(host_allowed(Some("127.0.0.1:47823"), 47823));
        assert!(host_allowed(Some("localhost:47823"), 47823));
        assert!(!host_allowed(Some("localhost:47824"), 47823));
        assert!(!host_allowed(Some("attacker.com:47823"), 47823));
        assert!(!host_allowed(None, 47823));
    }

    #[test]
    fn token_compare() {
        assert!(token_matches(Some("abc"), "abc"));
        assert!(!token_matches(Some("abd"), "abc"));
        assert!(!token_matches(Some("ab"), "abc"));
        assert!(!token_matches(None, "abc"));
        assert!(!token_matches(Some(""), ""), "empty token never matches");
    }

    #[test]
    fn origin_rules() {
        assert!(origin_allowed(b"chrome-extension://abcdef", None));
        assert!(origin_allowed(b"chrome-extension://abcdef", Some("")));
        assert!(!origin_allowed(b"chrome-extension://", None));
        assert!(!origin_allowed(b"https://evil.example", None));
        assert!(origin_allowed(b"chrome-extension://abcdef", Some("abcdef")));
        assert!(!origin_allowed(b"chrome-extension://other", Some("abcdef")));
    }

    #[tokio::test]
    async fn cors_echoes_extension_origin_until_pinned() {
        let s = serve().await;
        let preflight = |origin: &'static str| {
            s.http
                .request(Method::OPTIONS, s.url("/capture"))
                .header("origin", origin)
                .header("access-control-request-method", "POST")
                .send()
        };
        let allow = |res: &reqwest::Response| {
            res.headers()
                .get("access-control-allow-origin")
                .map(|v| v.to_str().unwrap().to_string())
        };
        let res = preflight("chrome-extension://abc").await.unwrap();
        assert_eq!(allow(&res).as_deref(), Some("chrome-extension://abc"));
        let res = preflight("https://evil.example").await.unwrap();
        assert_eq!(allow(&res), None);
        settings::set(&s.db, EXTENSION_ID_KEY, "pinned").unwrap();
        let res = preflight("chrome-extension://abc").await.unwrap();
        assert_eq!(allow(&res), None);
        let res = preflight("chrome-extension://pinned").await.unwrap();
        assert_eq!(allow(&res).as_deref(), Some("chrome-extension://pinned"));
    }

    #[tokio::test]
    async fn token_rotation_is_immediate() {
        let s = serve().await;
        let res = s.get("/buckets", Some(&s.token)).send().await.unwrap();
        assert_eq!(res.status(), 200);
        let new = rotate_token(&s.db).unwrap();
        assert_ne!(new, s.token);
        let res = s.get("/buckets", Some(&s.token)).send().await.unwrap();
        assert_eq!(res.status(), 401, "old token must stop working");
        let res = s.get("/buckets", Some(&new)).send().await.unwrap();
        assert_eq!(res.status(), 200);
        let code = pairing_code(&s.db).unwrap();
        assert!(code.ends_with(&format!(":{new}")), "{code}");
    }

    #[tokio::test]
    async fn oversized_body_is_413() {
        let s = serve().await;
        let body = json!({"url": local_url(&"a".repeat(20 * 1024))});
        let res = s
            .post("/capture", Some(&s.token), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 413);
    }

    #[tokio::test]
    async fn long_url_or_title_is_400() {
        let s = serve().await;
        let body = json!({"url": local_url(&"a".repeat(9 * 1024))});
        let res = s
            .post("/capture", Some(&s.token), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400);
        let body = json!({"url": local_url("t"), "title": "t".repeat(1025)});
        let res = s
            .post("/capture", Some(&s.token), body)
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400);
        assert!(items::list(&s.db, items::ItemFilter::All, true)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn pair_allow_returns_token_and_port() {
        let mut s = serve().await;
        let req = s
            .post("/pair", None, json!({"name": "Stow for Chrome"}))
            .send();
        let pending = tokio::spawn(req);
        let r = s.pair_requests.recv().await.unwrap();
        assert_eq!(r.name, "Stow for Chrome");
        assert!(s.pairing.respond(&r.id, true));
        let res = pending.await.unwrap().unwrap();
        assert_eq!(res.status(), 200);
        let v: Value = res.json().await.unwrap();
        assert_eq!(v, json!({"token": s.token, "port": s.port}));
    }

    #[tokio::test]
    async fn pair_deny_is_403() {
        let mut s = serve().await;
        let pending = tokio::spawn(s.post("/pair", None, json!({"name": "x"})).send());
        let r = s.pair_requests.recv().await.unwrap();
        assert!(s.pairing.respond(&r.id, false));
        let res = pending.await.unwrap().unwrap();
        assert_eq!(res.status(), 403);
        assert!(!res.text().await.unwrap().contains(&s.token));
    }

    #[tokio::test]
    async fn pair_timeout_is_403_and_frees_slot() {
        let mut s = serve_with_timeout(Duration::from_millis(100)).await;
        let res = s.post("/pair", None, json!({})).send().await.unwrap();
        assert_eq!(res.status(), 403);
        let r = s.pair_requests.recv().await.unwrap();
        assert_eq!(r.name, "Browser extension");
        assert!(!s.pairing.respond(&r.id, true), "expired");
        assert!(s.pairing.begin().is_some(), "slot freed");
    }

    #[tokio::test]
    async fn second_pair_request_is_429() {
        let mut s = serve().await;
        let first = tokio::spawn(s.post("/pair", None, json!({"name": "a"})).send());
        let r = s.pair_requests.recv().await.unwrap();
        let res = s
            .post("/pair", None, json!({"name": "b"}))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 429);
        s.pairing.respond(&r.id, false);
        assert_eq!(first.await.unwrap().unwrap().status(), 403);
    }

    #[tokio::test]
    async fn pair_abandoned_by_client_frees_slot() {
        let mut s = serve().await;
        let res = s
            .post("/pair", None, json!({"name": "a"}))
            .timeout(Duration::from_millis(150))
            .send()
            .await;
        assert!(res.is_err(), "client gave up");
        let _ = s.pair_requests.recv().await.unwrap();
        // The server notices the closed connection when it drops the handler future.
        let mut freed = false;
        for _ in 0..40 {
            if let Some((id, _rx)) = s.pairing.begin() {
                s.pairing.cancel(&id);
                freed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(freed, "abandoned request must not block pairing forever");
    }

    #[tokio::test]
    async fn pair_name_too_long_is_400() {
        let s = serve().await;
        let res = s
            .post("/pair", None, json!({"name": "n".repeat(101)}))
            .send()
            .await
            .unwrap();
        assert_eq!(res.status(), 400);
    }
}
