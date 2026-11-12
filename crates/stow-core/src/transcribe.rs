//! Whisper transcription (whisper-rs) and model download with progress.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::error::{IoContext, Result, StowError};

/// One timed piece of a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// Start offset.
    pub start_ms: i64,
    /// End offset.
    pub end_ms: i64,
    /// Recognized text.
    pub text: String,
}

/// Whisper output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    /// Segments in time order.
    pub segments: Vec<Segment>,
}

impl Transcript {
    /// One line per non-empty segment: `**[mm:ss]** text` (`h:mm:ss` from one hour).
    pub fn to_markdown(&self) -> String {
        self.segments
            .iter()
            .filter_map(|s| {
                let text = s.text.trim();
                (!text.is_empty()).then(|| format!("**[{}]** {text}", format_ts(s.start_ms)))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn format_ts(ms: i64) -> String {
    let total = ms.max(0) / 1000;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

/// `<data_dir>/models/ggml-<name>.bin`
pub fn model_path(data_dir: &Path, name: &str) -> PathBuf {
    data_dir.join("models").join(format!("ggml-{name}.bin"))
}

/// A downloadable Whisper model and what its file must hash to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelSpec {
    /// Name as stored in the `whisper_model` setting, e.g. `base.en`.
    pub name: &'static str,
    /// Exact file size.
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the file.
    pub sha256: &'static str,
}

/// The models Stow offers, with sizes and SHA-256 from the Hugging Face LFS metadata
/// of [`MODEL_COMMIT`].
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        name: "tiny.en",
        bytes: 77_704_715,
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
    },
    ModelSpec {
        name: "base.en",
        bytes: 147_964_211,
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
    },
    ModelSpec {
        name: "small.en",
        bytes: 487_614_201,
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
    },
];

/// The [`MODELS`] entry for `name`.
pub fn model_spec(name: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.name == name)
}

/// `ggerganov/whisper.cpp` commit the download URL is pinned to, so the files cannot
/// change underneath the hashes in [`MODELS`].
pub const MODEL_COMMIT: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// Where `ggml-<name>.bin` models are downloaded from (pinned to [`MODEL_COMMIT`]).
pub const MODEL_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1";

/// Download URL for a model name such as `base.en`.
pub fn model_url(name: &str) -> String {
    model_url_from(MODEL_BASE_URL, name)
}

fn model_url_from(base_url: &str, name: &str) -> String {
    format!("{}/ggml-{name}.bin", base_url.trim_end_matches('/'))
}

/// HTTP client for multi-hundred-megabyte model downloads: the shared
/// `fetch::client()` has a 15 s total timeout, which no model download survives.
/// This one bounds only connect and per-read stalls.
pub fn download_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(crate::fetch::USER_AGENT)
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?)
}

fn transcribe_err(e: impl std::fmt::Display) -> StowError {
    StowError::Transcribe(e.to_string())
}

/// Return the model path, downloading to `<path>.part` first when it is missing. The
/// download is checked against the [`MODELS`] size and SHA-256 while it streams and is
/// only renamed into place when both match; otherwise the `.part` is deleted and the
/// error is `Transcribe("checksum mismatch")`. An existing file is trusted (it got there
/// through that check). `on_progress(received_bytes, total_bytes)` fires per chunk.
pub async fn ensure_model(
    client: &reqwest::Client,
    data_dir: &Path,
    name: &str,
    on_progress: impl Fn(u64, Option<u64>) + Send + Sync,
) -> Result<PathBuf> {
    let spec = model_spec(name)
        .ok_or_else(|| StowError::Transcribe(format!("unknown whisper model {name:?}")))?;
    ensure_model_with_spec(client, MODEL_BASE_URL, data_dir, spec, on_progress).await
}

/// [`ensure_model`] for an explicit spec and mirror (tests serve fake models).
pub(crate) async fn ensure_model_with_spec(
    client: &reqwest::Client,
    base_url: &str,
    data_dir: &Path,
    spec: &ModelSpec,
    on_progress: impl Fn(u64, Option<u64>) + Send + Sync,
) -> Result<PathBuf> {
    let path = model_path(data_dir, spec.name);
    let url = model_url_from(base_url, spec.name);
    ensure_file(client, &url, &path, spec, on_progress).await?;
    Ok(path)
}

/// Serializes downloads so two transcriptions started together do not both write
/// the same `.part`; the second caller finds the finished file once the first is done.
static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Download `url` to `path` via `<path>.part` unless `path` already exists.
async fn ensure_file(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    spec: &ModelSpec,
    on_progress: impl Fn(u64, Option<u64>) + Send + Sync,
) -> Result<()> {
    let _guard = DOWNLOAD_LOCK.lock().await;
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .io_ctx("create dir", parent)?;
    }
    let part = PathBuf::from(format!("{}.part", path.display()));
    if let Err(e) = download(client, url, &part, spec, &on_progress).await {
        if let Err(rm) = tokio::fs::remove_file(&part).await {
            if rm.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(part = %part.display(), "could not remove partial model: {rm}");
            }
        }
        return Err(e);
    }
    tokio::fs::rename(&part, path)
        .await
        .map_err(|e| transcribe_err(format!("rename model: {e}")))
}

async fn download(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    spec: &ModelSpec,
    on_progress: &(impl Fn(u64, Option<u64>) + Send + Sync),
) -> Result<()> {
    use sha2::Digest;
    let mut resp = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| transcribe_err(format!("download {url}: {e}")))?;
    let total = resp.content_length();
    if total.is_some_and(|t| t != spec.bytes) {
        return Err(StowError::Transcribe("checksum mismatch".into()));
    }
    let mut file = tokio::fs::File::create(part)
        .await
        .map_err(|e| transcribe_err(format!("create {}: {e}", part.display())))?;
    let mut hasher = sha2::Sha256::new();
    let mut received = 0u64;
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| transcribe_err(format!("download {url}: {e}")))?
    {
        received += chunk.len() as u64;
        if received > spec.bytes {
            return Err(StowError::Transcribe("checksum mismatch".into()));
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|e| transcribe_err(format!("write model: {e}")))?;
        on_progress(received, total);
    }
    file.flush()
        .await
        .map_err(|e| transcribe_err(format!("write model: {e}")))?;
    if received != spec.bytes {
        return Err(StowError::Transcribe(format!(
            "incomplete download: {received} of {} bytes",
            spec.bytes
        )));
    }
    if crate::export::hex(&hasher.finalize()) != spec.sha256 {
        return Err(StowError::Transcribe("checksum mismatch".into()));
    }
    Ok(())
}

/// Caches the most recently loaded Whisper model so repeated transcriptions (a
/// re-transcribe, a queue of memos) do not reload hundreds of megabytes each time. Holds
/// one model: loading a different path replaces it. Loading happens under the lock, so
/// two callers never load the same model twice.
#[derive(Default)]
pub struct WhisperModelStore {
    loaded: std::sync::Mutex<Option<(PathBuf, Arc<WhisperContext>)>>,
}

impl std::fmt::Debug for WhisperModelStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let path = self
            .loaded
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|(p, _)| p.clone()));
        f.debug_struct("WhisperModelStore")
            .field("loaded", &path)
            .finish()
    }
}

impl WhisperModelStore {
    /// An empty store.
    pub fn new() -> WhisperModelStore {
        WhisperModelStore::default()
    }

    /// The context for `model`, loading it (blocking) if it is not the cached one.
    pub fn get(&self, model: &Path) -> Result<Arc<WhisperContext>> {
        let mut slot = self
            .loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((path, ctx)) = slot.as_ref() {
            if path == model {
                return Ok(ctx.clone());
            }
        }
        *slot = None;
        let ctx = Arc::new(load_model(model)?);
        *slot = Some((model.to_path_buf(), ctx.clone()));
        Ok(ctx)
    }

    /// Path of the cached model, if any.
    pub fn loaded_path(&self) -> Option<PathBuf> {
        self.loaded
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|(p, _)| p.clone())
    }

    /// [`transcribe_wav`] with the cached context. Blocking.
    pub fn transcribe(&self, model: &Path, wav: &Path) -> Result<Transcript> {
        let samples = load_samples(wav)?;
        let ctx = self.get(model)?;
        transcribe_samples(&ctx, &samples)
    }
}

fn load_model(model: &Path) -> Result<WhisperContext> {
    WhisperContext::new_with_params(model, WhisperContextParameters::default())
        .map_err(|e| transcribe_err(format!("load model {}: {e}", model.display())))
}

/// 16 kHz mono samples, padded to the ~1 s whisper.cpp needs.
fn load_samples(wav: &Path) -> Result<Vec<f32>> {
    let mut samples = crate::audio::read_wav_16k_f32(wav)?;
    let min_len = crate::audio::TARGET_HZ as usize;
    if samples.len() < min_len {
        samples.resize(min_len, 0.0);
    }
    Ok(samples)
}

/// Transcribe a WAV (any format; converted to 16 kHz mono) with a ggml Whisper model,
/// loading the model for this call only (see [`WhisperModelStore`] to reuse it).
pub fn transcribe_wav(model: &Path, wav: &Path) -> Result<Transcript> {
    let samples = load_samples(wav)?;
    transcribe_samples(&load_model(model)?, &samples)
}

fn transcribe_samples(ctx: &WhisperContext, samples: &[f32]) -> Result<Transcript> {
    let mut state = ctx.create_state().map_err(transcribe_err)?;

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some("en"));
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_token_timestamps(false);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get().min(8));
    params.set_n_threads(i32::try_from(threads).unwrap_or(4));

    state.full(params, samples).map_err(transcribe_err)?;

    let mut segments = Vec::new();
    for i in 0..state.full_n_segments() {
        let Some(seg) = state.get_segment(i) else {
            continue;
        };
        let text = seg.to_str_lossy().map_err(transcribe_err)?.into_owned();
        // whisper timestamps are in 10 ms units.
        segments.push(Segment {
            start_ms: seg.start_timestamp() * 10,
            end_ms: seg.end_timestamp() * 10,
            text,
        });
    }
    Ok(Transcript { segments })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn transcript_markdown_format() {
        let t = Transcript {
            segments: vec![
                Segment {
                    start_ms: 0,
                    end_ms: 1_500,
                    text: "  Hello there. ".into(),
                },
                Segment {
                    start_ms: 1_500,
                    end_ms: 2_000,
                    text: "   ".into(),
                },
                Segment {
                    start_ms: 61_000,
                    end_ms: 65_000,
                    text: "One minute in.".into(),
                },
                Segment {
                    start_ms: 3_661_000,
                    end_ms: 3_670_000,
                    text: "An hour in.".into(),
                },
            ],
        };
        assert_eq!(
            t.to_markdown(),
            "**[00:00]** Hello there.\n**[01:01]** One minute in.\n**[1:01:01]** An hour in."
        );
        assert_eq!(Transcript { segments: vec![] }.to_markdown(), "");
    }

    #[test]
    fn model_path_and_url() {
        assert_eq!(
            model_path(Path::new("/tmp/stow"), "base.en"),
            PathBuf::from("/tmp/stow/models/ggml-base.en.bin")
        );
        assert_eq!(
            model_url("base.en"),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base.en.bin"
        );
    }

    #[test]
    fn model_table_covers_offered_models() {
        let names: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
        assert_eq!(names, crate::repo::settings::WHISPER_MODELS);
        for m in MODELS {
            assert_eq!(m.sha256.len(), 64, "{}", m.name);
            assert!(m
                .sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
            assert!(m.bytes > 50_000_000, "{}", m.name);
            assert_eq!(model_spec(m.name), Some(m));
        }
        assert_eq!(
            model_spec("tiny.en").map(|m| m.sha256),
            Some("921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f")
        );
        assert_eq!(model_spec("large"), None);
    }

    #[test]
    fn model_store_does_not_cache_failed_loads() {
        let dir = tempfile::tempdir().unwrap();
        let bogus = dir.path().join("ggml-bogus.bin");
        std::fs::write(&bogus, b"not a model").unwrap();
        let store = WhisperModelStore::new();
        assert!(store.get(&bogus).is_err());
        assert_eq!(store.loaded_path(), None);
    }

    #[tokio::test]
    async fn ensure_model_rejects_unknown_name() {
        let dir = tempfile::tempdir().unwrap();
        let err = ensure_model(&reqwest::Client::new(), dir.path(), "large", |_, _| {})
            .await
            .unwrap_err();
        assert!(matches!(err, StowError::Transcribe(_)), "{err:?}");
    }

    fn sha256_hex(b: &[u8]) -> &'static str {
        use sha2::Digest;
        Box::leak(crate::export::hex(&sha2::Sha256::digest(b)).into_boxed_str())
    }

    fn spec_for(name: &'static str, body: &[u8]) -> ModelSpec {
        ModelSpec {
            name,
            bytes: body.len() as u64,
            sha256: sha256_hex(body),
        }
    }

    #[tokio::test]
    async fn ensure_model_rejects_checksum_mismatch() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let body = vec![7u8; 10_000];
        Mock::given(method("GET"))
            .and(path("/ggml-x.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let mut spec = spec_for("x", &body);
        spec.sha256 = sha256_hex(b"something else");
        let err = ensure_model_with_spec(
            &download_client().unwrap(),
            &server.uri(),
            dir.path(),
            &spec,
            |_, _| {},
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, StowError::Transcribe(m) if m == "checksum mismatch"),
            "{err:?}"
        );
        let dest = model_path(dir.path(), "x");
        assert!(!dest.exists(), "unverified model kept");
        assert!(
            !PathBuf::from(format!("{}.part", dest.display())).exists(),
            ".part left behind"
        );
    }

    fn part_of(dest: &Path) -> PathBuf {
        PathBuf::from(format!("{}.part", dest.display()))
    }

    #[tokio::test]
    async fn ensure_model_404_leaves_no_part_file() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ggml-x.bin"))
            .respond_with(ResponseTemplate::new(404).set_body_string("not here"))
            .mount(&server)
            .await;
        let dir = tempfile::tempdir().unwrap();
        let spec = spec_for("x", &[1u8; 1000]);
        let err = ensure_model_with_spec(
            &download_client().unwrap(),
            &server.uri(),
            dir.path(),
            &spec,
            |_, _| panic!("no progress for a failed request"),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(&err, StowError::Transcribe(m) if m.contains("404")),
            "{err:?}"
        );
        assert_eq!(err.code(), "transcribe");
        let dest = model_path(dir.path(), "x");
        assert!(!dest.exists());
        assert!(!part_of(&dest).exists(), ".part left behind");
        let leftovers = std::fs::read_dir(dest.parent().unwrap()).map_or(0, Iterator::count);
        assert_eq!(leftovers, 0, "models dir must be empty");
    }

    /// Serve one raw HTTP response per accepted connection, closing each socket after
    /// its bytes are written (so a client that expects more sees EOF, not a stall).
    async fn one_shot_server(responses: Vec<Vec<u8>>) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for resp in responses {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut req = vec![0u8; 4096];
                let _ = sock.read(&mut req).await;
                let _ = sock.write_all(&resp).await;
                let _ = sock.shutdown().await;
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn ensure_model_truncated_download_removes_part_and_next_call_retries() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        // Chunked body that ends cleanly after 10 of the 1000 promised bytes → the
        // stream is "complete" but short: "incomplete download", no .part.
        let short_chunked = b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
            Transfer-Encoding: chunked\r\nConnection: close\r\n\r\na\r\n0123456789\r\n0\r\n\r\n"
            .to_vec();
        // Content-Length 1000 but the connection closes after 10 bytes → transport error.
        let short_fixed = b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\n\
            Content-Length: 1000\r\nConnection: close\r\n\r\n0123456789"
            .to_vec();
        let body = vec![9u8; 1000];
        let spec = spec_for("x", &body);
        let dir = tempfile::tempdir().unwrap();
        let dest = model_path(dir.path(), "x");
        let client = download_client().unwrap();

        let base = one_shot_server(vec![short_chunked]).await;
        let err = ensure_model_with_spec(&client, &base, dir.path(), &spec, |_, _| {})
            .await
            .unwrap_err();
        assert!(
            matches!(&err, StowError::Transcribe(m) if m.contains("incomplete download") && m.contains("10 of 1000")),
            "{err:?}"
        );
        assert!(!dest.exists());
        assert!(
            !part_of(&dest).exists(),
            ".part left behind after short body"
        );

        let base = one_shot_server(vec![short_fixed]).await;
        let err = ensure_model_with_spec(&client, &base, dir.path(), &spec, |_, _| {})
            .await
            .unwrap_err();
        assert!(matches!(&err, StowError::Transcribe(_)), "{err:?}");
        assert!(!dest.exists());
        assert!(
            !part_of(&dest).exists(),
            ".part left behind after cut connection"
        );

        // Nothing is cached about the failures: the next call downloads for real.
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ggml-x.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .expect(1)
            .mount(&server)
            .await;
        let got = ensure_model_with_spec(&client, &server.uri(), dir.path(), &spec, |_, _| {})
            .await
            .unwrap();
        assert_eq!(got, dest);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!part_of(&dest).exists());
    }

    #[tokio::test]
    async fn ensure_model_returns_existing_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let p = model_path(dir.path(), "tiny.en");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, b"fake").unwrap();
        let client = reqwest::Client::new();
        let got = ensure_model(&client, dir.path(), "tiny.en", |_, _| {})
            .await
            .unwrap();
        assert_eq!(got, p);
    }

    #[tokio::test]
    async fn ensure_model_downloads_via_part_file_with_progress() {
        use std::sync::{Arc, Mutex};
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = vec![7u8; 10_000];
        Mock::given(method("GET"))
            .and(path("/ggml-x.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let dest = model_path(dir.path(), "x");
        let part = PathBuf::from(format!("{}.part", dest.display()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let got = ensure_model_with_spec(
            &download_client().unwrap(),
            &server.uri(),
            dir.path(),
            &spec_for("x", &body),
            move |r, t| {
                seen2.lock().unwrap().push((r, t));
            },
        )
        .await
        .unwrap_or_else(|e| panic!("download: {e}"));

        assert_eq!(got, dest);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(!part.exists());
        let seen = seen.lock().unwrap();
        assert!(!seen.is_empty());
        assert_eq!(seen.last().unwrap().0, 10_000);
        assert_eq!(seen.last().unwrap().1, Some(10_000));
    }

    #[tokio::test]
    async fn ensure_model_concurrent_calls_download_once() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        let body = vec![3u8; 50_000];
        Mock::given(method("GET"))
            .and(path("/ggml-tiny.en.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
            .expect(1)
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let client = download_client().unwrap();
        let base = server.uri();
        let spec = spec_for("tiny.en", &body);
        let (a, b) = tokio::join!(
            ensure_model_with_spec(&client, &base, dir.path(), &spec, |_, _| {}),
            ensure_model_with_spec(&client, &base, dir.path(), &spec, |_, _| {}),
        );
        let dest = model_path(dir.path(), "tiny.en");
        assert_eq!(a.unwrap(), dest);
        assert_eq!(b.unwrap(), dest);
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        assert!(
            !PathBuf::from(format!("{}.part", dest.display())).exists(),
            ".part left behind"
        );
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            1,
            "model downloaded twice"
        );
    }

    /// Real whisper.cpp run. Needs a ggml model and a 16 kHz WAV of someone saying
    /// "The quick brown fox jumps over the lazy dog":
    /// `STOW_WHISPER_MODEL=... STOW_TEST_WAV=... cargo test -p stow-core transcribe_real_model -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a real ggml model and WAV (see doc comment)"]
    fn transcribe_real_model() {
        let (Ok(model), Ok(wav)) = (
            std::env::var("STOW_WHISPER_MODEL"),
            std::env::var("STOW_TEST_WAV"),
        ) else {
            panic!("set STOW_WHISPER_MODEL and STOW_TEST_WAV to run");
        };
        let started = std::time::Instant::now();
        let t = transcribe_wav(Path::new(&model), Path::new(&wav)).unwrap();
        let elapsed = started.elapsed();
        let md = t.to_markdown();
        eprintln!(
            "--- transcript ({elapsed:?}) ---\n{md}\n--- segments: {:?}",
            t.segments
        );

        assert!(!t.segments.is_empty());
        let text = t
            .segments
            .iter()
            .map(|s| s.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(text.contains("quick brown fox"), "transcript: {text:?}");
        let first = &t.segments[0];
        assert_eq!(first.start_ms, 0, "first segment start");
        assert!(first.end_ms > 0, "first segment end {}", first.end_ms);
        for s in &t.segments {
            assert!(s.end_ms >= s.start_ms);
        }
        assert!(md.starts_with("**[00:00]**"), "{md}");
    }
}
