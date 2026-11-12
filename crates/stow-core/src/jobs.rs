//! In-process background jobs. Network and heavy work never block the capture path.

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use tokio::sync::{mpsc, Notify, Semaphore};

use crate::fetch::{metadata, snapshot};
use crate::models::JobStatus;
use crate::repo::items;
use crate::{Db, Result, StowError};

/// A unit of background work.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Job {
    /// Fetch title/description/favicon/preview for an item.
    FetchMetadata {
        /// Item id.
        item_id: String,
    },
    /// Fetch and store a readable snapshot of an item.
    FetchSnapshot {
        /// Item id.
        item_id: String,
    },
    /// Transcribe a recording with Whisper.
    Transcribe {
        /// Recording id.
        recording_id: String,
    },
    /// Execute an AI run.
    Ai {
        /// AI run id.
        run_id: String,
    },
}

/// Something the UI should refresh.
#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    /// Buckets were created/renamed/deleted/reordered (from any window or the server).
    BucketsChanged,
    /// A new item row exists (metadata/snapshot may still be pending).
    ItemAdded(String),
    /// An item's metadata, snapshot, or bucket changed.
    ItemUpdated(String),
    /// A recording's transcript landed or failed.
    RecordingUpdated(String),
    /// An AI run finished or failed.
    AiRunUpdated(String),
    /// Model download progress.
    Progress {
        /// What is downloading (e.g. `whisper_model`).
        kind: String,
        /// Bytes received so far.
        received: u64,
        /// Total bytes, when the server said.
        total: Option<u64>,
    },
}

/// Receives every [`JobEvent`]; the app forwards them to windows.
pub type JobHook = Arc<dyn Fn(JobEvent) + Send + Sync>;

/// Settings for [`JobQueue::start_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JobQueueOptions {
    /// Fetch URLs on `localhost` and private/link-local addresses. Off by default: a
    /// saved link must not be able to make Stow probe the local network.
    pub allow_local_urls: bool,
}

/// Handle to the job worker. Cheap to clone.
#[derive(Clone)]
pub struct JobQueue {
    tx: mpsc::Sender<Job>,
    hook: JobHook,
    opts: JobQueueOptions,
    state: Arc<QueueState>,
}

/// Jobs that may wait in the channel; beyond this `enqueue` drops and warns.
const QUEUE_CAPACITY: usize = 1024;
/// Concurrent page/metadata fetches.
const NETWORK_SLOTS: usize = 4;
/// Concurrent Whisper runs; each already uses up to 8 threads and a model in memory.
const TRANSCRIBE_SLOTS: usize = 1;
/// Concurrent AI provider calls.
const AI_SLOTS: usize = 2;
/// How long [`JobQueue::shutdown`] waits for running jobs.
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Executes one job. The real one dispatches to the runners below; tests inject fakes.
pub(crate) type RunFn =
    Arc<dyn Fn(Job) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send + Sync>;

/// Shared between the handle clones and the worker.
struct QueueState {
    /// Jobs queued or running; an identical job is not queued twice.
    inflight: Mutex<HashSet<Job>>,
    accepting: AtomicBool,
    /// Jobs handed to a task and not yet finished (waiting for a slot or running).
    active: AtomicUsize,
    idle: Notify,
    network: Semaphore,
    transcribe: Semaphore,
    ai: Semaphore,
}

impl QueueState {
    fn new() -> QueueState {
        QueueState {
            inflight: Mutex::new(HashSet::new()),
            accepting: AtomicBool::new(true),
            active: AtomicUsize::new(0),
            idle: Notify::new(),
            network: Semaphore::new(NETWORK_SLOTS),
            transcribe: Semaphore::new(TRANSCRIBE_SLOTS),
            ai: Semaphore::new(AI_SLOTS),
        }
    }

    fn inflight(&self) -> std::sync::MutexGuard<'_, HashSet<Job>> {
        self.inflight.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn slots(&self, job: &Job) -> &Semaphore {
        match job {
            Job::FetchMetadata { .. } | Job::FetchSnapshot { .. } => &self.network,
            Job::Transcribe { .. } => &self.transcribe,
            Job::Ai { .. } => &self.ai,
        }
    }
}

/// Clears a job's in-flight entry and active count when its task ends, even by panic.
struct Finished {
    state: Arc<QueueState>,
    job: Job,
}

impl Drop for Finished {
    fn drop(&mut self) {
        self.state.inflight().remove(&self.job);
        if self.state.active.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.state.idle.notify_waiters();
        }
    }
}

impl std::fmt::Debug for JobQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobQueue")
            .field("opts", &self.opts)
            .finish_non_exhaustive()
    }
}

impl JobQueue {
    /// Build the shared HTTP client and spawn the worker on the current tokio runtime,
    /// with default [`JobQueueOptions`] (local addresses are not fetched).
    pub fn start(db: Arc<Db>, hook: JobHook) -> Result<JobQueue> {
        Self::start_with(db, hook, JobQueueOptions::default())
    }

    /// [`JobQueue::start`] with explicit options. Jobs run concurrently up to a per-kind
    /// limit: 4 network fetches, 1 transcription, 2 AI calls.
    pub fn start_with(db: Arc<Db>, hook: JobHook, opts: JobQueueOptions) -> Result<JobQueue> {
        let client = crate::fetch::client_for(opts.allow_local_urls)?;
        #[cfg(feature = "transcribe")]
        let models = Arc::new(crate::transcribe::WhisperModelStore::new());
        let job_hook = hook.clone();
        let run: RunFn = Arc::new(move |job| {
            let (db, client, hook) = (db.clone(), client.clone(), job_hook.clone());
            #[cfg(feature = "transcribe")]
            let models = models.clone();
            Box::pin(async move {
                let ctx = RunCtx {
                    db: &db,
                    client: &client,
                    hook: &hook,
                    #[cfg(feature = "transcribe")]
                    models: &models,
                };
                run(job, ctx).await
            })
        });
        Ok(Self::spawn_worker(run, hook, opts))
    }

    /// Spawn the dispatch loop around `run`.
    pub(crate) fn spawn_worker(run: RunFn, hook: JobHook, opts: JobQueueOptions) -> JobQueue {
        let (tx, mut rx) = mpsc::channel::<Job>(QUEUE_CAPACITY);
        let state = Arc::new(QueueState::new());
        let worker_state = state.clone();
        tokio::spawn(async move {
            while let Some(job) = rx.recv().await {
                let state = worker_state.clone();
                state.active.fetch_add(1, Ordering::SeqCst);
                let finished = Finished {
                    state: state.clone(),
                    job: job.clone(),
                };
                let run = run.clone();
                tokio::spawn(async move {
                    let _finished = finished;
                    let Ok(_slot) = state.slots(&job).acquire().await else {
                        return;
                    };
                    if !state.accepting.load(Ordering::SeqCst) {
                        tracing::debug!(?job, "queue shut down; job not started");
                        return;
                    }
                    if let Err(e) = run(job.clone()).await {
                        tracing::warn!(?job, "job failed: {e}");
                    }
                });
            }
        });
        JobQueue {
            tx,
            hook,
            opts,
            state,
        }
    }

    /// Fire an event through the same hook jobs use, for changes made outside a job
    /// (e.g. an item inserted by a command or the extension server).
    pub fn notify(&self, ev: JobEvent) {
        (self.hook)(ev);
    }

    /// Queue a job. Returns `false` (and queues nothing) when an identical job is already
    /// queued or running, the queue is full (logged), or it was shut down.
    pub fn enqueue(&self, job: Job) -> bool {
        if !self.state.accepting.load(Ordering::SeqCst) {
            tracing::debug!(?job, "queue shut down; job dropped");
            return false;
        }
        if !self.state.inflight().insert(job.clone()) {
            return false;
        }
        match self.tx.try_send(job) {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(job)) => {
                tracing::warn!(?job, "job queue full; dropping job");
                self.state.inflight().remove(&job);
                false
            }
            Err(mpsc::error::TrySendError::Closed(job)) => {
                tracing::error!("job queue worker is gone");
                self.state.inflight().remove(&job);
                false
            }
        }
    }

    /// Stop accepting jobs and wait up to 5 s for running ones to finish. Queued jobs
    /// that have not started are dropped (they stay `pending` in the database, so
    /// `resume_pending` picks them up next launch). Returns `true` when everything
    /// finished in time.
    pub async fn shutdown(&self) -> bool {
        self.state.accepting.store(false, Ordering::SeqCst);
        let drained = async {
            loop {
                let idle = self.state.idle.notified();
                tokio::pin!(idle);
                idle.as_mut().enable();
                if self.state.active.load(Ordering::SeqCst) == 0 {
                    return;
                }
                idle.await;
            }
        };
        let done = tokio::time::timeout(SHUTDOWN_GRACE, drained).await.is_ok();
        if !done {
            tracing::warn!("jobs still running after {SHUTDOWN_GRACE:?}; shutting down anyway");
        }
        done
    }

    /// A queue whose jobs land on the returned receiver instead of a worker.
    #[cfg(test)]
    pub(crate) fn for_tests(hook: JobHook) -> (JobQueue, mpsc::Receiver<Job>) {
        Self::for_tests_with(hook, JobQueueOptions::default())
    }

    /// [`JobQueue::for_tests`] with explicit options.
    #[cfg(test)]
    pub(crate) fn for_tests_with(
        hook: JobHook,
        opts: JobQueueOptions,
    ) -> (JobQueue, mpsc::Receiver<Job>) {
        let (tx, rx) = mpsc::channel(QUEUE_CAPACITY);
        let state = Arc::new(QueueState::new());
        (
            JobQueue {
                tx,
                hook,
                opts,
                state,
            },
            rx,
        )
    }

    /// Whether this queue fetches local/private addresses (see [`JobQueueOptions`]).
    pub fn allow_local_urls(&self) -> bool {
        self.opts.allow_local_urls
    }

    /// A hook that ignores every event.
    pub fn noop_hook() -> JobHook {
        Arc::new(|_| {})
    }
}

/// A snapshot or transcription that has started this many times is not retried again.
pub const MAX_ATTEMPTS: i64 = 3;

/// After a clean quit nothing is really running: put `running` rows (snapshots,
/// transcriptions, AI runs) back to `pending` so [`resume_pending`] re-queues them next
/// launch instead of failing them as interrupted. Returns how many rows changed.
pub fn requeue_running(db: &Db) -> Result<usize> {
    db.tx(|t| {
        let items = t.execute(
            "UPDATE items SET snapshot_status = 'pending', snapshot_error = NULL, updated_at = ?1 \
             WHERE snapshot_status = 'running'",
            [crate::ids::now()],
        )?;
        let recordings = t.execute(
            "UPDATE recordings SET transcript_status = 'pending', transcript_error = NULL \
             WHERE transcript_status = 'running'",
            [],
        )?;
        let ai = t.execute(
            "UPDATE ai_runs SET status = 'pending', error = NULL WHERE status = 'running'",
            [],
        )?;
        Ok(items + recordings + ai)
    })
}

/// Recover after a restart. Rows still `running` were cut off mid-job (possibly by the
/// job itself crashing the process), so they are marked failed with `interrupted` and
/// their attempt counter bumped instead of being retried blindly. `pending` snapshots and
/// transcriptions that already used [`MAX_ATTEMPTS`] fail with `gave up after 3
/// attempts`; every other `pending` snapshot, transcription, and AI run is re-enqueued.
/// Items whose metadata never landed ([`items::needs_metadata`]: pending ones, and
/// recent videos, which never get a snapshot) get a metadata job first. Returns the jobs
/// enqueued, in order.
pub fn resume_pending(db: &Db, queue: &JobQueue) -> Result<Vec<Job>> {
    let gave_up = format!("gave up after {MAX_ATTEMPTS} attempts");
    let jobs = db.tx(|t| {
        let now = crate::ids::now();
        let since = (chrono::Utc::now() - chrono::Duration::days(items::METADATA_RETRY_DAYS))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        t.execute(
            "UPDATE items SET snapshot_status = 'failed', snapshot_error = 'interrupted', \
             snapshot_attempts = snapshot_attempts + 1, updated_at = ?1 \
             WHERE snapshot_status = 'running'",
            [&now],
        )?;
        t.execute(
            "UPDATE recordings SET transcript_status = 'failed', transcript_error = 'interrupted', \
             transcript_attempts = transcript_attempts + 1 WHERE transcript_status = 'running'",
            [],
        )?;
        t.execute(
            "UPDATE ai_runs SET status = 'failed', error = 'interrupted' WHERE status = 'running'",
            [],
        )?;
        t.execute(
            "UPDATE items SET snapshot_status = 'failed', snapshot_error = ?1, updated_at = ?2 \
             WHERE snapshot_status = 'pending' AND snapshot_attempts >= ?3",
            rusqlite::params![gave_up, now, MAX_ATTEMPTS],
        )?;
        t.execute(
            "UPDATE recordings SET transcript_status = 'failed', transcript_error = ?1 \
             WHERE transcript_status = 'pending' AND transcript_attempts >= ?2",
            rusqlite::params![gave_up, MAX_ATTEMPTS],
        )?;
        let pending_ids = |sql: &str| -> Result<Vec<String>> {
            let mut st = t.prepare(sql)?;
            let v = st
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(v)
        };
        let mut jobs: Vec<Job> = Vec::new();
        for it in items::list_resumable_conn(t, &since)? {
            if items::needs_metadata(&it, &now) {
                jobs.push(Job::FetchMetadata {
                    item_id: it.id.clone(),
                });
            }
            if it.snapshot_status == JobStatus::Pending {
                jobs.push(Job::FetchSnapshot { item_id: it.id });
            }
        }
        jobs.extend(
            pending_ids(
                "SELECT id FROM recordings WHERE transcript_status = 'pending' ORDER BY created_at, id",
            )?
            .into_iter()
            .map(|recording_id| Job::Transcribe { recording_id }),
        );
        jobs.extend(
            pending_ids("SELECT id FROM ai_runs WHERE status = 'pending' ORDER BY created_at, id")?
                .into_iter()
                .map(|run_id| Job::Ai { run_id }),
        );
        Ok(jobs)
    })?;
    for job in &jobs {
        queue.enqueue(job.clone());
    }
    Ok(jobs)
}

/// What a job needs from the queue.
struct RunCtx<'a> {
    db: &'a Db,
    client: &'a reqwest::Client,
    hook: &'a JobHook,
    #[cfg(feature = "transcribe")]
    models: &'a Arc<crate::transcribe::WhisperModelStore>,
}

async fn run(job: Job, ctx: RunCtx<'_>) -> Result<()> {
    let RunCtx {
        db, client, hook, ..
    } = ctx;
    match job {
        Job::FetchMetadata { item_id } => run_fetch_metadata(db, client, &item_id, hook).await,
        Job::FetchSnapshot { item_id } => {
            run_fetch_snapshot(db, client, &item_id, hook).await;
            Ok(())
        }
        #[cfg(feature = "transcribe")]
        Job::Transcribe { recording_id } => {
            run_transcribe_with(db, ctx.models.clone(), &recording_id, hook).await
        }
        #[cfg(not(feature = "transcribe"))]
        Job::Transcribe { recording_id } => transcribe_unavailable(db, &recording_id, hook),
        #[cfg(feature = "ai")]
        Job::Ai { run_id } => crate::ai::run_ai(db, &run_id, hook).await,
        #[cfg(not(feature = "ai"))]
        Job::Ai { run_id } => ai_unavailable(db, &run_id, hook),
    }
}

/// Without the `ai` feature every AI run fails with a clear message.
#[cfg(not(feature = "ai"))]
fn ai_unavailable(db: &Db, run_id: &str, hook: &JobHook) -> Result<()> {
    crate::repo::ai_runs::fail(db, run_id, "AI is not available in this build")?;
    hook(JobEvent::AiRunUpdated(run_id.to_string()));
    Ok(())
}

fn parse_url(s: &str) -> Result<url::Url> {
    url::Url::parse(s).map_err(|e| StowError::InvalidUrl {
        input: s.to_string(),
        reason: e.to_string(),
    })
}

/// Fetch title/description/site/favicon/image for an item. Errors leave the item untouched.
/// An item deleted while the fetch ran is not an error: the files just written are
/// removed again and nothing is reported.
#[tracing::instrument(skip_all, fields(item_id))]
pub(crate) async fn run_fetch_metadata(
    db: &Db,
    client: &reqwest::Client,
    item_id: &str,
    hook: &JobHook,
) -> Result<()> {
    tracing::Span::current().record("item_id", item_id);
    let it = match items::get(db, item_id) {
        Err(StowError::NotFound { .. }) => {
            tracing::debug!("item deleted before metadata fetch started");
            return Ok(());
        }
        r => r?,
    };
    let url = parse_url(&it.canonical_url)?;
    let meta = metadata::fetch_metadata(client, &url).await?;
    let assets = db.data_dir().join("assets");
    let fetch_asset = |url: Option<url::Url>, what: &'static str| {
        let assets = &assets;
        async move {
            let url = url?;
            match metadata::download_asset(client, &url, assets, &format!("{item_id}-{what}")).await
            {
                Ok(p) => Some(p),
                Err(e) => {
                    tracing::debug!(%url, "{what} not downloaded: {e}");
                    None
                }
            }
        }
    };
    let favicon_path = fetch_asset(meta.favicon_url.clone(), "favicon").await;
    let image_path = fetch_asset(meta.image_url.clone(), "image").await;
    let patch = items::MetaPatch {
        title: meta.title,
        description: meta.description,
        site_name: meta.site_name,
        favicon_path: favicon_path.map(|p| p.to_string_lossy().into_owned()),
        image_path: image_path.map(|p| p.to_string_lossy().into_owned()),
    };
    match items::set_metadata(db, item_id, &patch) {
        Err(StowError::NotFound { .. }) => {
            tracing::debug!("item deleted during metadata fetch; removing its assets");
            items::remove_own_files(db.data_dir(), item_id);
            return Ok(());
        }
        r => r?,
    };
    hook(JobEvent::ItemUpdated(item_id.to_string()));
    Ok(())
}

/// Fetch a readable snapshot. Always records the outcome on the item (done or failed +
/// error), unless the item was deleted meanwhile: then the snapshot files just written
/// are removed again and nothing is reported.
#[tracing::instrument(skip_all, fields(item_id))]
pub(crate) async fn run_fetch_snapshot(
    db: &Db,
    client: &reqwest::Client,
    item_id: &str,
    hook: &JobHook,
) {
    tracing::Span::current().record("item_id", item_id);
    let result: Result<()> = async {
        let it = items::get(db, item_id)?;
        if it.kind == crate::models::ItemKind::Video {
            // A readable snapshot of a video page is just player chrome; skip it.
            items::set_snapshot_status(db, item_id, JobStatus::None, None)?;
            return Ok(());
        }
        items::start_snapshot(db, item_id)?;
        let url = parse_url(&it.canonical_url)?;
        let snap = snapshot::fetch_snapshot(client, &url).await?;
        snapshot::write_snapshot(db.data_dir(), item_id, &snap)?;
        let title = snap.title.clone().or(it.title.clone()).unwrap_or_default();
        items::finish_snapshot(db, item_id, &title, &snap.text)
    }
    .await;
    match result {
        Ok(()) => {}
        Err(StowError::NotFound { .. }) => {
            tracing::debug!("item deleted during snapshot fetch; removing its files");
            items::remove_own_files(db.data_dir(), item_id);
            return;
        }
        Err(e) => {
            let msg = e.to_string();
            tracing::debug!("snapshot failed: {msg}");
            match items::set_snapshot_status(db, item_id, JobStatus::Failed, Some(&msg)) {
                Ok(()) => {}
                Err(StowError::NotFound { .. }) => return,
                Err(se) => tracing::warn!("could not record snapshot failure: {se}"),
            }
        }
    }
    hook(JobEvent::ItemUpdated(item_id.to_string()));
}

/// Transcribe a recording: make sure the Whisper model is present (downloading with
/// progress events), run whisper.cpp on a blocking thread, store the Markdown transcript.
/// The download uses `transcribe::download_client()`, not the 15 s shared client.
#[cfg(feature = "transcribe")]
#[cfg(test)]
pub(crate) async fn run_transcribe(db: &Db, recording_id: &str, hook: &JobHook) -> Result<()> {
    let models = Arc::new(crate::transcribe::WhisperModelStore::new());
    run_transcribe_with(db, models, recording_id, hook).await
}

/// [`run_transcribe`] reusing models already loaded in `models`.
#[cfg(feature = "transcribe")]
#[tracing::instrument(skip_all, fields(recording_id))]
pub(crate) async fn run_transcribe_with(
    db: &Db,
    models: Arc<crate::transcribe::WhisperModelStore>,
    recording_id: &str,
    hook: &JobHook,
) -> Result<()> {
    use crate::repo::{recordings, settings};
    tracing::Span::current().record("recording_id", recording_id);
    let rec = recordings::get(db, recording_id)?;
    recordings::start_transcript(db, recording_id)?;
    hook(JobEvent::RecordingUpdated(recording_id.to_string()));
    let outcome: Result<String> = async {
        let model_name = settings::get_key(db, settings::Key::WhisperModel)?;
        let model_name = settings::validate_whisper_model(&model_name)?;
        let progress_hook = hook.clone();
        let client = crate::transcribe::download_client()?;
        let model = crate::transcribe::ensure_model(
            &client,
            db.data_dir(),
            model_name,
            move |received, total| {
                progress_hook(JobEvent::Progress {
                    kind: "whisper_model".into(),
                    received,
                    total,
                });
            },
        )
        .await?;
        let wav = recordings::resolve_audio_path(db.data_dir(), &rec.audio_path);
        let transcript = tokio::task::spawn_blocking(move || models.transcribe(&model, &wav))
            .await
            .map_err(|e| StowError::Transcribe(format!("worker panicked: {e}")))??;
        Ok(transcript.to_markdown())
    }
    .await;
    let result = match outcome {
        Ok(md) => recordings::set_transcript(db, recording_id, &md).map(drop),
        Err(e) => {
            if let Err(se) =
                recordings::set_status(db, recording_id, JobStatus::Failed, Some(&e.to_string()))
            {
                tracing::warn!(recording_id, "could not record transcription failure: {se}");
            }
            Err(e)
        }
    };
    hook(JobEvent::RecordingUpdated(recording_id.to_string()));
    result
}

/// Without the `transcribe` feature every transcription fails with a clear message.
#[cfg(not(feature = "transcribe"))]
fn transcribe_unavailable(db: &Db, recording_id: &str, hook: &JobHook) -> Result<()> {
    let err = StowError::Transcribe("transcription is not available in this build".into());
    crate::repo::recordings::set_status(
        db,
        recording_id,
        JobStatus::Failed,
        Some(&err.to_string()),
    )?;
    hook(JobEvent::RecordingUpdated(recording_id.to_string()));
    Err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::JobStatus;
    use crate::repo::{items, search, test_db};
    use crate::Db;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const BARE: &str = include_str!("../tests/fixtures/bare_title.html");
    const READABLE: &str = include_str!("../tests/fixtures/readable.html");

    fn client() -> reqwest::Client {
        crate::fetch::client_allow_local().unwrap()
    }

    fn item(db: &Db, url: &str) -> crate::models::Item {
        items::create(
            db,
            items::NewItem {
                url: url.into(),
                bucket_id: None,
                title: None,
            },
        )
        .unwrap()
        .value
    }

    #[tokio::test]
    async fn fetch_metadata_job_sets_title_and_emits_event() {
        let (_d, db) = test_db();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/post"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(BARE, "text/html"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/favicon.ico"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(vec![1u8, 2], "image/png"))
            .mount(&server)
            .await;
        let it = item(&db, &format!("{}/post", server.uri()));
        let (tx, rx) = std::sync::mpsc::channel();
        let hook: JobHook = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        run_fetch_metadata(&db, &client(), &it.id, &hook)
            .await
            .unwrap();
        let got = items::get(&db, &it.id).unwrap();
        assert_eq!(got.title.as_deref(), Some("Just a title"));
        let fav = got.favicon_path.expect("favicon downloaded");
        assert!(
            fav.ends_with(&format!("assets/{}-favicon.png", it.id)),
            "{fav}"
        );
        assert!(std::path::Path::new(&fav).is_file());
        assert!(matches!(rx.try_recv(), Ok(JobEvent::ItemUpdated(id)) if id == it.id));
    }

    #[tokio::test]
    async fn fetch_metadata_job_failure_keeps_item_and_host_title() {
        let (_d, db) = test_db();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let it = item(&db, &format!("{}/x", server.uri()));
        let err = run_fetch_metadata(&db, &client(), &it.id, &JobQueue::noop_hook()).await;
        assert!(err.is_err());
        let got = items::get(&db, &it.id).unwrap();
        assert_eq!(got.title.as_deref(), Some("127.0.0.1"));
    }

    #[tokio::test]
    async fn fetch_snapshot_job_skips_videos() {
        let (_d, db) = test_db();
        let it = item(&db, "https://youtu.be/abc123");
        items::set_snapshot_status(&db, &it.id, JobStatus::Pending, None).unwrap();
        run_fetch_snapshot(&db, &client(), &it.id, &JobQueue::noop_hook()).await;
        let got = items::get(&db, &it.id).unwrap();
        assert_eq!(got.snapshot_status, JobStatus::None);
        assert!(got.snapshot_error.is_none());
    }

    #[tokio::test]
    async fn fetch_snapshot_job_failure_sets_failed_with_error() {
        let (_d, db) = test_db();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let it = item(&db, &format!("{}/gone", server.uri()));
        run_fetch_snapshot(&db, &client(), &it.id, &JobQueue::noop_hook()).await;
        let got = items::get(&db, &it.id).unwrap();
        assert_eq!(got.snapshot_status, JobStatus::Failed);
        assert!(got.snapshot_error.unwrap().contains("404"));
    }

    #[tokio::test]
    async fn fetch_snapshot_job_success_indexes_text() {
        let (_d, db) = test_db();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(READABLE, "text/html"))
            .mount(&server)
            .await;
        let it = item(&db, &format!("{}/a", server.uri()));
        run_fetch_snapshot(&db, &client(), &it.id, &JobQueue::noop_hook()).await;
        let got = items::get(&db, &it.id).unwrap();
        assert_eq!(
            got.snapshot_status,
            JobStatus::Done,
            "{:?}",
            got.snapshot_error
        );
        assert!(got.snapshot_fetched_at.is_some());
        let md = crate::fetch::snapshot::read_snapshot_md(db.data_dir(), &it.id)
            .unwrap()
            .unwrap();
        assert!(md.contains("zebra crossing"));
        let hits = search::query(&db, "zebra", 10).unwrap();
        assert!(hits
            .iter()
            .any(|h| h.doc_type == "snapshot" && h.item_id.as_deref() == Some(it.id.as_str())));
    }

    #[cfg(feature = "transcribe")]
    #[tokio::test]
    async fn transcribe_job_failure_marks_recording_failed() {
        use crate::models::ParentType;
        use crate::repo::{recordings, settings};
        let (_d, db) = test_db();
        let it = item(&db, "https://example.com/rec");
        // Point the model at a nonexistent local server so ensure_model fails fast.
        settings::set(&db, "whisper_model", "tiny.en").unwrap();
        std::fs::write(
            db.data_dir().join("models").join("ggml-tiny.en.bin"),
            b"not a model",
        )
        .unwrap();
        let wav = db.data_dir().join("audio").join("r.wav");
        crate::audio::write_wav_16k(&wav, &vec![0.0f32; 16000]).unwrap();
        let rec =
            recordings::create(&db, ParentType::Item, &it.id, wav.to_str().unwrap(), 1000).unwrap();
        let r = run_transcribe(&db, &rec.id, &JobQueue::noop_hook()).await;
        assert!(r.is_err());
        let got = recordings::get(&db, &rec.id).unwrap();
        assert_eq!(got.transcript_status, JobStatus::Failed);
        assert!(got.transcript_error.is_some());
    }

    #[cfg(not(feature = "transcribe"))]
    #[test]
    fn transcribe_without_feature_fails_recording_clearly() {
        use crate::models::ParentType;
        use crate::repo::recordings;
        let (_d, db) = test_db();
        let b = crate::repo::test_bucket(&db, "B");
        let rec = recordings::create(&db, ParentType::Bucket, &b.id, "/x.wav", 1).unwrap();
        assert!(transcribe_unavailable(&db, &rec.id, &JobQueue::noop_hook()).is_err());
        let got = recordings::get(&db, &rec.id).unwrap();
        assert_eq!(got.transcript_status, JobStatus::Failed);
        assert!(got.transcript_error.unwrap().contains("not available"));
    }

    #[tokio::test]
    async fn resume_pending_enqueues_unfinished_work() {
        use crate::models::{AiAction, ParentType, Provider};
        use crate::repo::{ai_runs, recordings};
        let (_d, db) = test_db();
        let pending = item(&db, "https://example.com/pending");
        items::set_snapshot_status(&db, &pending.id, JobStatus::Pending, None).unwrap();
        let running = item(&db, "https://example.com/running");
        items::set_snapshot_status(&db, &running.id, JobStatus::Running, None).unwrap();
        let done = item(&db, "https://example.com/done");
        items::set_snapshot_done(&db, &done.id).unwrap();
        let failed = item(&db, "https://example.com/failed");
        items::set_snapshot_status(&db, &failed.id, JobStatus::Failed, Some("x")).unwrap();
        let _untouched = item(&db, "https://example.com/none");

        let rec_pending =
            recordings::create(&db, ParentType::Item, &pending.id, "/a.wav", 1).unwrap();
        let rec_running =
            recordings::create(&db, ParentType::Item, &pending.id, "/b.wav", 1).unwrap();
        recordings::set_status(&db, &rec_running.id, JobStatus::Running, None).unwrap();
        let rec_done = recordings::create(&db, ParentType::Item, &pending.id, "/c.wav", 1).unwrap();
        recordings::set_transcript(&db, &rec_done.id, "done").unwrap();

        let ai_pending = ai_runs::create(
            &db,
            ParentType::Item,
            &pending.id,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "m",
            "h1",
        )
        .unwrap();
        let ai_running = ai_runs::create(
            &db,
            ParentType::Item,
            &pending.id,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "m",
            "h2",
        )
        .unwrap();
        ai_runs::start(&db, &ai_running.id).unwrap();
        let ai_failed = ai_runs::create(
            &db,
            ParentType::Item,
            &pending.id,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "m",
            "h3",
        )
        .unwrap();
        ai_runs::fail(&db, &ai_failed.id, "boom").unwrap();

        let (q, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        let jobs = resume_pending(&db, &q).unwrap();

        // Rows left `running` were interrupted mid-flight: failed, not re-queued. The
        // pending item still has its host placeholder title, so metadata is re-fetched too.
        let expected = vec![
            Job::FetchMetadata {
                item_id: pending.id.clone(),
            },
            Job::FetchSnapshot {
                item_id: pending.id.clone(),
            },
            Job::Transcribe {
                recording_id: rec_pending.id.clone(),
            },
            Job::Ai {
                run_id: ai_pending.id.clone(),
            },
        ];
        assert_eq!(jobs, expected);
        assert_eq!(drain(&mut rx), expected, "every returned job was enqueued");
    }

    fn attempts(db: &Db, table: &str, col: &str, id: &str) -> i64 {
        db.with(|c| {
            Ok(c.query_row(
                &format!("SELECT {col} FROM {table} WHERE id = ?1"),
                [id],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    fn set_attempts(db: &Db, table: &str, col: &str, id: &str, n: i64) {
        db.with(|c| {
            c.execute(
                &format!("UPDATE {table} SET {col} = ?2 WHERE id = ?1"),
                rusqlite::params![id, n],
            )?;
            Ok(())
        })
        .unwrap();
    }

    fn drain(rx: &mut mpsc::Receiver<Job>) -> Vec<Job> {
        let mut v = Vec::new();
        while let Ok(j) = rx.try_recv() {
            v.push(j);
        }
        v
    }

    #[tokio::test]
    async fn resume_marks_running_as_interrupted() {
        use crate::models::{AiAction, ParentType, Provider};
        use crate::repo::{ai_runs, recordings};
        let (_d, db) = test_db();
        let running = item(&db, "https://example.com/running");
        items::set_snapshot_status(&db, &running.id, JobStatus::Running, None).unwrap();
        let rec = recordings::create(&db, ParentType::Item, &running.id, "/b.wav", 1).unwrap();
        recordings::set_status(&db, &rec.id, JobStatus::Running, None).unwrap();
        let run = ai_runs::create(
            &db,
            ParentType::Item,
            &running.id,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "m",
            "h",
        )
        .unwrap();
        ai_runs::start(&db, &run.id).unwrap();

        let (q, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        let jobs = resume_pending(&db, &q).unwrap();
        assert!(jobs.is_empty(), "running rows are not re-queued: {jobs:?}");
        assert!(drain(&mut rx).is_empty());

        let it = items::get(&db, &running.id).unwrap();
        assert_eq!(it.snapshot_status, JobStatus::Failed);
        assert_eq!(it.snapshot_error.as_deref(), Some("interrupted"));
        assert_eq!(attempts(&db, "items", "snapshot_attempts", &running.id), 1);
        let r = recordings::get(&db, &rec.id).unwrap();
        assert_eq!(r.transcript_status, JobStatus::Failed);
        assert_eq!(r.transcript_error.as_deref(), Some("interrupted"));
        assert_eq!(
            attempts(&db, "recordings", "transcript_attempts", &rec.id),
            1
        );
        let a = ai_runs::get(&db, &run.id).unwrap();
        assert_eq!(a.status, JobStatus::Failed);
        assert_eq!(a.error.as_deref(), Some("interrupted"));
    }

    #[tokio::test]
    async fn resume_gives_up_after_three_attempts() {
        use crate::models::ParentType;
        use crate::repo::recordings;
        let (_d, db) = test_db();
        let retry = item(&db, "https://example.com/retry");
        items::set_snapshot_status(&db, &retry.id, JobStatus::Pending, None).unwrap();
        set_attempts(&db, "items", "snapshot_attempts", &retry.id, 2);
        let tired = item(&db, "https://example.com/tired");
        items::set_snapshot_status(&db, &tired.id, JobStatus::Pending, None).unwrap();
        set_attempts(&db, "items", "snapshot_attempts", &tired.id, 3);
        let rec = recordings::create(&db, ParentType::Item, &retry.id, "/a.wav", 1).unwrap();
        set_attempts(&db, "recordings", "transcript_attempts", &rec.id, 3);

        let (q, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        let jobs = resume_pending(&db, &q).unwrap();
        let expected = vec![
            Job::FetchMetadata {
                item_id: retry.id.clone(),
            },
            Job::FetchSnapshot {
                item_id: retry.id.clone(),
            },
        ];
        assert_eq!(jobs, expected);
        assert_eq!(drain(&mut rx), expected);

        let t = items::get(&db, &tired.id).unwrap();
        assert_eq!(t.snapshot_status, JobStatus::Failed);
        assert_eq!(
            t.snapshot_error.as_deref(),
            Some("gave up after 3 attempts")
        );
        let r = recordings::get(&db, &rec.id).unwrap();
        assert_eq!(r.transcript_status, JobStatus::Failed);
        assert_eq!(
            r.transcript_error.as_deref(),
            Some("gave up after 3 attempts")
        );
        assert_eq!(
            items::get(&db, &retry.id).unwrap().snapshot_status,
            JobStatus::Pending
        );
    }

    #[tokio::test]
    async fn fetch_snapshot_job_counts_attempts() {
        let (_d, db) = test_db();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let it = item(&db, &format!("{}/gone", server.uri()));
        run_fetch_snapshot(&db, &client(), &it.id, &JobQueue::noop_hook()).await;
        run_fetch_snapshot(&db, &client(), &it.id, &JobQueue::noop_hook()).await;
        assert_eq!(attempts(&db, "items", "snapshot_attempts", &it.id), 2);
    }

    #[tokio::test]
    async fn enqueue_dedupes_identical_jobs() {
        let (q, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        let a = Job::FetchSnapshot {
            item_id: "a".into(),
        };
        assert!(q.enqueue(a.clone()));
        assert!(!q.enqueue(a.clone()), "identical job already queued");
        assert!(q.enqueue(Job::FetchMetadata {
            item_id: "a".into()
        }));
        assert_eq!(drain(&mut rx).len(), 2);

        // With a worker, the job can be queued again once it has finished.
        let (done_tx, mut done_rx) = tokio::sync::mpsc::unbounded_channel();
        let run: RunFn = Arc::new(move |job| {
            let done_tx = done_tx.clone();
            Box::pin(async move {
                let _ = done_tx.send(job);
                Ok(())
            })
        });
        let q = JobQueue::spawn_worker(run, JobQueue::noop_hook(), JobQueueOptions::default());
        assert!(q.enqueue(a.clone()));
        tokio::time::timeout(std::time::Duration::from_secs(5), done_rx.recv())
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !q.enqueue(a.clone()) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("in-flight entry cleared after the job finished");
    }

    /// Per job kind: (running now, peak).
    type Concurrency =
        Arc<std::sync::Mutex<std::collections::HashMap<&'static str, (usize, usize)>>>;

    /// A runner that records the peak number of concurrently running jobs per kind.
    fn concurrency_probe() -> (RunFn, Concurrency, Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let stats: Concurrency = Arc::default();
        let finished = Arc::new(AtomicUsize::new(0));
        let (s, f) = (stats.clone(), finished.clone());
        let run: RunFn = Arc::new(move |job| {
            let (stats, finished) = (s.clone(), f.clone());
            Box::pin(async move {
                let kind = match job {
                    Job::FetchMetadata { .. } | Job::FetchSnapshot { .. } => "network",
                    Job::Transcribe { .. } => "transcribe",
                    Job::Ai { .. } => "ai",
                };
                {
                    let mut m = stats.lock().unwrap();
                    let e = m.entry(kind).or_insert((0, 0));
                    e.0 += 1;
                    e.1 = e.1.max(e.0);
                }
                tokio::time::sleep(std::time::Duration::from_millis(60)).await;
                stats.lock().unwrap().get_mut(kind).unwrap().0 -= 1;
                finished.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        (run, stats, finished)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn transcribe_jobs_run_one_at_a_time() {
        use std::sync::atomic::Ordering;
        let (run, stats, finished) = concurrency_probe();
        let q = JobQueue::spawn_worker(run, JobQueue::noop_hook(), JobQueueOptions::default());
        let mut n = 0;
        for i in 0..3 {
            assert!(q.enqueue(Job::Transcribe {
                recording_id: format!("r{i}")
            }));
            n += 1;
        }
        for i in 0..5 {
            q.enqueue(Job::Ai {
                run_id: format!("a{i}"),
            });
            n += 1;
        }
        for i in 0..8 {
            q.enqueue(Job::FetchSnapshot {
                item_id: format!("i{i}"),
            });
            n += 1;
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while finished.load(Ordering::SeqCst) < n {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("all jobs finished");
        let m = stats.lock().unwrap();
        assert_eq!(m["transcribe"].1, 1, "transcriptions overlapped");
        assert!(m["ai"].1 <= 2 && m["ai"].1 >= 1, "ai peak {}", m["ai"].1);
        assert!(
            m["network"].1 <= 4 && m["network"].1 >= 2,
            "network peak {}",
            m["network"].1
        );
    }

    #[tokio::test]
    async fn shutdown_waits_for_inflight() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let done = Arc::new(AtomicBool::new(false));
        let (started_tx, mut started_rx) = tokio::sync::mpsc::unbounded_channel();
        let d = done.clone();
        let run: RunFn = Arc::new(move |_job| {
            let (done, started_tx) = (d.clone(), started_tx.clone());
            Box::pin(async move {
                let _ = started_tx.send(());
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                done.store(true, Ordering::SeqCst);
                Ok(())
            })
        });
        let q = JobQueue::spawn_worker(run, JobQueue::noop_hook(), JobQueueOptions::default());
        assert!(q.enqueue(Job::Ai { run_id: "x".into() }));
        started_rx.recv().await.unwrap();
        assert!(q.shutdown().await, "drained within the deadline");
        assert!(
            done.load(Ordering::SeqCst),
            "shutdown returned before the job finished"
        );
        assert!(
            !q.enqueue(Job::Ai { run_id: "y".into() }),
            "no new jobs after shutdown"
        );
    }

    #[tokio::test]
    async fn fetch_snapshot_job_times_out_and_marks_failed() {
        // A server that sends headers and then stalls forever must not hang the job: a
        // client timeout turns into `snapshot_status = failed` with the http error recorded.
        use crate::fetch::test_support::stalling_server;
        let (_d, db) = test_db();
        let url = stalling_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5000\r\n\r\n",
            vec![],
        )
        .await;
        let it = item(&db, &url);
        items::set_snapshot_status(&db, &it.id, JobStatus::Pending, None).unwrap();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(300))
            .build()
            .unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let hook: JobHook = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        let started = std::time::Instant::now();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_fetch_snapshot(&db, &client, &it.id, &hook),
        )
        .await
        .expect("job must give up on a stalling server");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
        let got = items::get(&db, &it.id).unwrap();
        assert_eq!(got.snapshot_status, JobStatus::Failed);
        let err = got.snapshot_error.expect("error recorded");
        assert!(err.starts_with("http error"), "{err}");
        assert_eq!(attempts(&db, "items", "snapshot_attempts", &it.id), 1);
        assert!(matches!(rx.try_recv(), Ok(JobEvent::ItemUpdated(id)) if id == it.id));
        assert!(
            crate::fetch::snapshot::read_snapshot_md(db.data_dir(), &it.id)
                .unwrap()
                .is_none(),
            "no snapshot files for a failed fetch"
        );
    }

    #[tokio::test]
    async fn fetch_metadata_job_times_out_and_leaves_item_untouched() {
        use crate::fetch::test_support::stalling_server;
        let (_d, db) = test_db();
        let url = stalling_server(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 5000\r\n\r\n",
            vec![b"<html><head><title>never finishes".to_vec()],
        )
        .await;
        let it = item(&db, &url);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(300))
            .build()
            .unwrap();
        let started = std::time::Instant::now();
        let err = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            run_fetch_metadata(&db, &client, &it.id, &JobQueue::noop_hook()),
        )
        .await
        .expect("job must give up on a stalling server")
        .unwrap_err();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "took {:?}",
            started.elapsed()
        );
        assert!(matches!(err, StowError::Http(_)), "{err:?}");
        assert_eq!(err.code(), "http");
        let got = items::get(&db, &it.id).unwrap();
        assert_eq!(got.title.as_deref(), Some("127.0.0.1"), "placeholder kept");
        assert_eq!(got.updated_at, it.updated_at, "row not touched");
    }

    /// A runner whose jobs block until `release` is notified, counting starts and ends.
    fn gated_runner() -> (
        RunFn,
        Arc<Notify>,
        Arc<std::sync::atomic::AtomicUsize>,
        Arc<std::sync::atomic::AtomicUsize>,
    ) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let release = Arc::new(Notify::new());
        let started = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let (r, s, f) = (release.clone(), started.clone(), finished.clone());
        let run: RunFn = Arc::new(move |_job| {
            let (release, started, finished) = (r.clone(), s.clone(), f.clone());
            Box::pin(async move {
                started.fetch_add(1, Ordering::SeqCst);
                release.notified().await;
                finished.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
        (run, release, started, finished)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_during_long_job_waits_for_it_and_starts_nothing_else() {
        use std::sync::atomic::Ordering;
        let (run, release, started, finished) = gated_runner();
        let q = JobQueue::spawn_worker(run, JobQueue::noop_hook(), JobQueueOptions::default());
        // One transcription slot: the first job runs, the second waits for the slot.
        assert!(q.enqueue(Job::Transcribe {
            recording_id: "long".into()
        }));
        assert!(q.enqueue(Job::Transcribe {
            recording_id: "queued".into()
        }));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while started.load(Ordering::SeqCst) < 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("first job started");
        assert_eq!(
            started.load(Ordering::SeqCst),
            1,
            "second job waits for the slot"
        );

        let q2 = q.clone();
        let shutdown = tokio::spawn(async move { q2.shutdown().await });
        // Shutdown is waiting on the long job; meanwhile nothing new is accepted.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(!shutdown.is_finished(), "returned while a job was running");
        assert!(
            !q.enqueue(Job::Ai {
                run_id: "late".into()
            }),
            "accepting after shutdown"
        );
        assert_eq!(finished.load(Ordering::SeqCst), 0);

        // Let the long job finish: shutdown drains, and the queued job is dropped unstarted.
        release.notify_waiters();
        let drained = tokio::time::timeout(std::time::Duration::from_secs(5), shutdown)
            .await
            .expect("shutdown returned")
            .unwrap();
        assert!(drained, "everything that was running finished in time");
        assert_eq!(finished.load(Ordering::SeqCst), 1);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert_eq!(
            started.load(Ordering::SeqCst),
            1,
            "a job that had not started when shutdown began must never start"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "waits the full 5 s SHUTDOWN_GRACE on purpose"]
    async fn shutdown_gives_up_on_a_job_that_outlives_the_grace_period() {
        use std::sync::atomic::Ordering;
        let (run, release, started, finished) = gated_runner();
        let q = JobQueue::spawn_worker(run, JobQueue::noop_hook(), JobQueueOptions::default());
        assert!(q.enqueue(Job::Ai {
            run_id: "stuck".into()
        }));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while started.load(Ordering::SeqCst) < 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let t = std::time::Instant::now();
        let drained = q.shutdown().await;
        let waited = t.elapsed();
        assert!(!drained, "must report that a job was still running");
        assert!(
            waited >= SHUTDOWN_GRACE && waited < SHUTDOWN_GRACE + std::time::Duration::from_secs(2),
            "waited {waited:?}"
        );
        assert_eq!(finished.load(Ordering::SeqCst), 0, "job still running");
        release.notify_waiters();
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while finished.load(Ordering::SeqCst) < 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("the abandoned job still completes when released");
    }

    #[tokio::test]
    async fn resume_pending_enqueues_1000_rows_quickly() {
        let (_d, db) = test_db();
        let mut ids = Vec::with_capacity(1000);
        for i in 0..1000 {
            let it = item(&db, &format!("https://example.com/pending/{i}"));
            ids.push(it.id);
        }
        // Metadata already landed (real title, favicon): only the snapshots are pending.
        db.with(|c| {
            c.execute(
                "UPDATE items SET snapshot_status = 'pending', title = 'Fetched', \
                 favicon_path = '/f.png'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        // Pad with rows resume_pending must skip quickly: done, failed, none.
        for (i, status) in [JobStatus::Done, JobStatus::Failed, JobStatus::None]
            .into_iter()
            .enumerate()
        {
            for j in 0..100 {
                let it = item(&db, &format!("https://example.com/other/{i}/{j}"));
                items::set_snapshot_status(&db, &it.id, status, None).unwrap();
            }
        }
        let (q, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        let t = std::time::Instant::now();
        let jobs = resume_pending(&db, &q).unwrap();
        let took = t.elapsed();
        eprintln!("perf: resume_pending with 1000 pending rows = {took:?}");
        assert_eq!(jobs.len(), 1000);
        assert!(
            took < std::time::Duration::from_millis(500),
            "resume_pending took {took:?}"
        );
        let queued = drain(&mut rx);
        assert_eq!(queued.len(), 1000, "every job reached the channel");
        assert_eq!(queued, jobs);
        // Oldest first, as created.
        let order: Vec<&str> = jobs
            .iter()
            .map(|j| match j {
                Job::FetchSnapshot { item_id } => item_id.as_str(),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(order, ids.iter().map(String::as_str).collect::<Vec<_>>());
        // Running it again changes nothing: the jobs are in flight (deduped) and the rows
        // are still pending.
        let again = resume_pending(&db, &q).unwrap();
        assert_eq!(
            again.len(),
            1000,
            "rows are still pending until a worker runs them"
        );
        assert!(
            drain(&mut rx).is_empty(),
            "identical jobs are not queued twice"
        );
    }

    #[tokio::test]
    async fn notify_calls_hook_directly() {
        let (_d, db) = test_db();
        let (tx, rx) = std::sync::mpsc::channel();
        let hook: JobHook = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        let q = JobQueue::start(Arc::new(db), hook).unwrap();
        q.notify(JobEvent::ItemAdded("abc".into()));
        assert!(matches!(rx.try_recv(), Ok(JobEvent::ItemAdded(id)) if id == "abc"));
    }

    #[tokio::test]
    async fn queue_runs_enqueued_job_and_calls_hook() {
        let (_d, db) = test_db();
        let db = Arc::new(db);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/p"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(BARE, "text/html"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        let it = item(&db, &format!("{}/p", server.uri()));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let hook: JobHook = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        let q = JobQueue::start_with(
            db.clone(),
            hook,
            JobQueueOptions {
                allow_local_urls: true,
            },
        )
        .unwrap();
        q.enqueue(Job::FetchMetadata {
            item_id: it.id.clone(),
        });
        let ev = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(ev, JobEvent::ItemUpdated(ref id) if *id == it.id));
        assert_eq!(
            items::get(&db, &it.id).unwrap().title.as_deref(),
            Some("Just a title")
        );
    }

    /// Set `created_at` to `days` ago.
    fn age_item(db: &Db, id: &str, days: i64) {
        let then = (chrono::Utc::now() - chrono::Duration::days(days))
            .to_rfc3339_opts(chrono::SecondsFormat::Micros, true);
        db.with(|c| {
            c.execute(
                "UPDATE items SET created_at = ?2 WHERE id = ?1",
                rusqlite::params![id, then],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[tokio::test]
    async fn resume_enqueues_metadata_for_placeholder_titles() {
        let (_d, db) = test_db();
        // Pending snapshot, host placeholder title: metadata + snapshot.
        let placeholder = item(&db, "https://example.com/placeholder");
        items::mark_snapshot_pending(&db, &placeholder.id).unwrap();
        // Pending snapshot, real title but no favicon yet: metadata + snapshot.
        let no_favicon = item(&db, "https://example.com/no-favicon");
        items::mark_snapshot_pending(&db, &no_favicon.id).unwrap();
        items::set_metadata(
            &db,
            &no_favicon.id,
            &items::MetaPatch {
                title: Some("Real".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // Pending snapshot with metadata complete: snapshot only.
        let complete = item(&db, "https://example.com/complete");
        items::mark_snapshot_pending(&db, &complete.id).unwrap();
        items::set_metadata(
            &db,
            &complete.id,
            &items::MetaPatch {
                title: Some("Real".into()),
                favicon_path: Some("/f.png".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // Video with placeholder title (no snapshot ever): metadata only.
        let video = item(&db, "https://youtu.be/vid1");
        assert_eq!(video.snapshot_status, JobStatus::None);
        // Video with full metadata: nothing.
        let video_done = item(&db, "https://youtu.be/vid2");
        items::set_metadata(
            &db,
            &video_done.id,
            &items::MetaPatch {
                title: Some("Clip".into()),
                favicon_path: Some("/f.png".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // Video with placeholder title but older than the retry window: nothing.
        let old_video = item(&db, "https://youtu.be/vid3");
        age_item(&db, &old_video.id, items::METADATA_RETRY_DAYS + 1);
        // Placeholder title but snapshot never requested (status none, not video): nothing.
        let _untouched = item(&db, "https://example.com/none");
        // Snapshot done but metadata failed back then: nothing (not pending, not video).
        let done = item(&db, "https://example.com/done");
        items::set_snapshot_done(&db, &done.id).unwrap();

        let (q, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        let jobs = resume_pending(&db, &q).unwrap();
        let expected = vec![
            Job::FetchMetadata {
                item_id: placeholder.id.clone(),
            },
            Job::FetchSnapshot {
                item_id: placeholder.id.clone(),
            },
            Job::FetchMetadata {
                item_id: no_favicon.id.clone(),
            },
            Job::FetchSnapshot {
                item_id: no_favicon.id.clone(),
            },
            Job::FetchSnapshot {
                item_id: complete.id.clone(),
            },
            Job::FetchMetadata {
                item_id: video.id.clone(),
            },
        ];
        assert_eq!(jobs, expected);
        assert_eq!(drain(&mut rx), expected);
    }

    #[test]
    fn needs_metadata_predicate() {
        let (_d, db) = test_db();
        let now = crate::ids::now();
        let it = item(&db, "https://docs.example.com/x");
        assert!(items::needs_metadata(&it, &now), "host placeholder");
        let titled = items::set_metadata(
            &db,
            &it.id,
            &items::MetaPatch {
                title: Some("Guide".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            items::needs_metadata(&titled, &now),
            "favicon still missing"
        );
        let full = items::set_metadata(
            &db,
            &it.id,
            &items::MetaPatch {
                favicon_path: Some("/f.png".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!items::needs_metadata(&full, &now));
        let stale = item(&db, "https://example.com/stale");
        age_item(&db, &stale.id, items::METADATA_RETRY_DAYS + 1);
        let stale = items::get(&db, &stale.id).unwrap();
        assert!(
            !items::needs_metadata(&stale, &now),
            "old placeholders are left alone"
        );
    }

    /// A one-shot HTTP server that answers `body` only after `release` fires; `hit`
    /// resolves once the request has arrived.
    async fn gated_server(
        body: String,
    ) -> (String, tokio::sync::oneshot::Receiver<()>, Arc<Notify>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (hit_tx, hit_rx) = tokio::sync::oneshot::channel();
        let release = Arc::new(Notify::new());
        let rel = release.clone();
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut req = vec![0u8; 4096];
            let _ = sock.read(&mut req).await;
            let _ = hit_tx.send(());
            rel.notified().await;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\
                 Connection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.shutdown().await;
        });
        (format!("http://{addr}/page"), hit_rx, release)
    }

    fn search_docs_for(db: &Db, id: &str) -> i64 {
        db.with(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM search_docs WHERE doc_id = ?1 OR item_id = ?1",
                [id],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    }

    #[tokio::test]
    async fn snapshot_job_cleans_up_when_item_deleted_mid_fetch() {
        let (_d, db) = test_db();
        let (url, hit, release) = gated_server(READABLE.to_string()).await;
        let it = item(&db, &url);
        items::mark_snapshot_pending(&db, &it.id).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let hook: JobHook = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        let client = client();
        let job = run_fetch_snapshot(&db, &client, &it.id, &hook);
        let delete_mid_fetch = async {
            hit.await.unwrap();
            assert_eq!(
                items::get(&db, &it.id).unwrap().snapshot_status,
                JobStatus::Running
            );
            items::delete(&db, &it.id).unwrap();
            release.notify_one();
        };
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(job, delete_mid_fetch);
        })
        .await
        .expect("job finished");

        assert!(matches!(
            items::get(&db, &it.id),
            Err(StowError::NotFound { .. })
        ));
        let (html, md) = crate::fetch::snapshot::snapshot_paths(db.data_dir(), &it.id);
        assert!(!html.exists() && !md.exists(), "snapshot files removed");
        assert_eq!(search_docs_for(&db, &it.id), 0, "no orphan search docs");
        assert!(
            rx.try_recv().is_err(),
            "no ItemUpdated for an item that no longer exists"
        );
    }

    #[tokio::test]
    async fn metadata_job_cleans_up_when_item_deleted_mid_fetch() {
        let (_d, db) = test_db();
        let icons = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/favicon.ico"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(vec![1u8, 2], "image/png"))
            .mount(&icons)
            .await;
        let html = format!(
            "<html><head><title>Late</title><link rel=\"icon\" href=\"{}/favicon.ico\"></head>\
             <body>x</body></html>",
            icons.uri()
        );
        let (url, hit, release) = gated_server(html).await;
        let it = item(&db, &url);
        let (tx, rx) = std::sync::mpsc::channel();
        let hook: JobHook = Arc::new(move |ev| {
            let _ = tx.send(ev);
        });
        let client = client();
        let job = run_fetch_metadata(&db, &client, &it.id, &hook);
        let delete_mid_fetch = async {
            hit.await.unwrap();
            items::delete(&db, &it.id).unwrap();
            release.notify_one();
        };
        let (outcome, ()) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            tokio::join!(job, delete_mid_fetch)
        })
        .await
        .expect("job finished");
        outcome.expect("a vanished item is not a job failure");

        let leftovers: Vec<_> = std::fs::read_dir(db.data_dir().join("assets"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with(&it.id))
            .collect();
        assert!(leftovers.is_empty(), "assets removed: {leftovers:?}");
        assert!(matches!(
            items::get(&db, &it.id),
            Err(StowError::NotFound { .. })
        ));
        assert!(rx.try_recv().is_err(), "no event for a deleted item");
    }

    #[test]
    fn requeue_running_puts_running_rows_back_to_pending() {
        use crate::models::{AiAction, ParentType, Provider};
        use crate::repo::{ai_runs, recordings};
        let (_d, db) = test_db();
        let running = item(&db, "https://example.com/running");
        items::start_snapshot(&db, &running.id).unwrap();
        let done = item(&db, "https://example.com/done");
        items::set_snapshot_done(&db, &done.id).unwrap();
        let failed = item(&db, "https://example.com/failed");
        items::set_snapshot_status(&db, &failed.id, JobStatus::Failed, Some("x")).unwrap();
        let rec = recordings::create(&db, ParentType::Item, &running.id, "/a.wav", 1).unwrap();
        recordings::set_status(&db, &rec.id, JobStatus::Running, None).unwrap();
        let rec_done = recordings::create(&db, ParentType::Item, &running.id, "/b.wav", 1).unwrap();
        recordings::set_transcript(&db, &rec_done.id, "done").unwrap();
        let run = ai_runs::create(
            &db,
            ParentType::Item,
            &running.id,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "m",
            "h",
        )
        .unwrap();
        ai_runs::start(&db, &run.id).unwrap();

        assert_eq!(requeue_running(&db).unwrap(), 3);

        let it = items::get(&db, &running.id).unwrap();
        assert_eq!(it.snapshot_status, JobStatus::Pending);
        assert!(it.snapshot_error.is_none());
        assert_eq!(
            items::get(&db, &done.id).unwrap().snapshot_status,
            JobStatus::Done,
            "finished work is untouched"
        );
        assert_eq!(
            items::get(&db, &failed.id).unwrap().snapshot_status,
            JobStatus::Failed
        );
        assert_eq!(
            recordings::get(&db, &rec.id).unwrap().transcript_status,
            JobStatus::Pending
        );
        assert_eq!(
            recordings::get(&db, &rec_done.id)
                .unwrap()
                .transcript_status,
            JobStatus::Done
        );
        assert_eq!(
            ai_runs::get(&db, &run.id).unwrap().status,
            JobStatus::Pending
        );
        assert_eq!(requeue_running(&db).unwrap(), 0, "idempotent");

        // Next launch re-queues them instead of failing them as interrupted.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let (q, mut rx) = rt.block_on(async { JobQueue::for_tests(JobQueue::noop_hook()) });
        let jobs = resume_pending(&db, &q).unwrap();
        assert!(jobs.contains(&Job::FetchSnapshot {
            item_id: running.id.clone()
        }));
        assert!(jobs.contains(&Job::Transcribe {
            recording_id: rec.id.clone()
        }));
        assert!(jobs.contains(&Job::Ai {
            run_id: run.id.clone()
        }));
        assert_eq!(drain(&mut rx), jobs);
        assert_eq!(
            items::get(&db, &running.id).unwrap().snapshot_error,
            None,
            "not marked interrupted"
        );
    }
}
