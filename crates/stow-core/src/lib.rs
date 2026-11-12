//! Stow core: storage, capture, fetching, background jobs, AI, and audio. No UI
//! dependencies; the CLI and the Tauri app are thin shells over this crate.
//!
//! Start with [`Stow`] (open a data dir, capture URLs) and the plain functions in
//! [`repo`]. Every fallible call returns [`Result`] with a [`StowError`] whose
//! [`StowError::code`] is stable for frontends.
//!
//! Cargo features: `audio` (cpal + hound recording), `transcribe` (whisper-rs; implies
//! `audio`), `metal` (GPU whisper on macOS), `ai` (provider clients), `keychain`
//! (API keys in the OS keychain). All but `metal` are on by default.
#![forbid(unsafe_code)]

#[cfg(feature = "ai")]
pub mod ai;
#[cfg(feature = "audio")]
pub mod audio;
pub mod capture;
pub mod db;
pub mod doctor;
pub mod error;
pub mod export;
pub mod fetch;
pub mod ids;
pub mod jobs;
pub mod models;
pub mod repo;
pub mod scheduler;
#[cfg(feature = "transcribe")]
pub mod transcribe;

pub use db::Db;
pub use error::{EntityKind, Result, StowError};
pub use models::Created;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use jobs::{Job, JobEvent, JobQueue};
use models::Item;
use repo::items;

/// `~/Library/Application Support/Stow` on macOS; `None` when the platform has no data dir.
pub fn default_data_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("Stow"))
}

/// A URL to save, from any capture path (panel, paste, extension, import, CLI).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CaptureRequest {
    /// Raw URL; normalized and validated by [`Stow::capture`].
    pub url: String,
    /// Target bucket; `None` = Inbox.
    pub bucket_id: Option<String>,
    /// Title to use instead of the fetched one.
    pub title: Option<String>,
}

/// The library facade: a database plus, in the app, the job queue.
#[derive(Debug, Clone)]
pub struct Stow {
    db: Arc<Db>,
    jobs: Option<JobQueue>,
    allow_local_urls: bool,
}

/// `snapshot_error` for items whose host is local and `allow_local_urls` is off.
pub const LOCAL_NOT_FETCHED: &str = "local address not fetched";

impl Stow {
    /// Open (creating and migrating) the data dir, without a job queue.
    pub fn open(data_dir: &Path) -> Result<Stow> {
        Ok(Stow {
            db: Arc::new(Db::open(data_dir)?),
            jobs: None,
            allow_local_urls: false,
        })
    }

    /// Wrap an open database and a running job queue. `allow_local_urls` follows the
    /// queue's [`jobs::JobQueueOptions`].
    pub fn with_jobs(db: Arc<Db>, jobs: JobQueue) -> Stow {
        Stow {
            db,
            allow_local_urls: jobs.allow_local_urls(),
            jobs: Some(jobs),
        }
    }

    /// Fetch (or not) URLs on `localhost` and private addresses. Off by default; see
    /// [`jobs::JobQueueOptions::allow_local_urls`].
    #[must_use]
    pub fn with_allow_local_urls(mut self, allow: bool) -> Stow {
        self.allow_local_urls = allow;
        self
    }

    /// Whether local/private addresses are fetched.
    pub fn allow_local_urls(&self) -> bool {
        self.allow_local_urls
    }

    /// True (and the item marked failed with [`LOCAL_NOT_FETCHED`]) when the item's
    /// host is local and local fetching is off.
    fn skip_local(&self, item: &Item) -> Result<bool> {
        if self.allow_local_urls {
            return Ok(false);
        }
        let local =
            url::Url::parse(&item.canonical_url).is_ok_and(|u| fetch::net::is_local_url(&u));
        if local {
            items::set_snapshot_status(
                &self.db,
                &item.id,
                models::JobStatus::Failed,
                Some(LOCAL_NOT_FETCHED),
            )?;
        }
        Ok(local)
    }

    /// The database, for [`repo`] calls.
    pub fn db(&self) -> &Arc<Db> {
        &self.db
    }

    /// The job queue, when one is attached.
    pub fn jobs(&self) -> Option<&JobQueue> {
        self.jobs.as_ref()
    }

    /// Normalize → create (dedupe by canonical URL) → for a new item, enqueue metadata and
    /// snapshot jobs (when a queue is attached) → notify `ItemAdded`, or `ItemUpdated` for
    /// an existing one. The row is marked `snapshot_status = pending` before the jobs are
    /// queued, so a quit before they run is recovered by [`jobs::resume_pending`]. Videos
    /// get metadata only (no readable snapshot). A local URL (`localhost`, private IP) is
    /// saved but, unless `allow_local_urls`, never fetched: its snapshot is marked failed
    /// with [`LOCAL_NOT_FETCHED`].
    pub fn capture(&self, req: CaptureRequest) -> Result<Created<Item>> {
        let mut created = items::create(
            &self.db,
            items::NewItem {
                url: req.url,
                bucket_id: req.bucket_id,
                title: req.title,
            },
        )?;
        let skipped = created.created && self.skip_local(&created.value)?;
        if skipped {
            created.value = items::get(&self.db, &created.value.id)?;
        }
        if let Some(jobs) = &self.jobs {
            let id = created.value.id.clone();
            if created.created {
                if !skipped {
                    let snapshot = created.value.kind != models::ItemKind::Video;
                    if snapshot {
                        created.value = items::mark_snapshot_pending(&self.db, &id)?;
                    }
                    jobs.enqueue(Job::FetchMetadata {
                        item_id: id.clone(),
                    });
                    if snapshot {
                        jobs.enqueue(Job::FetchSnapshot {
                            item_id: id.clone(),
                        });
                    }
                }
                jobs.notify(JobEvent::ItemAdded(id));
            } else {
                jobs.notify(JobEvent::ItemUpdated(id));
            }
        }
        Ok(created)
    }

    /// Fetch metadata and the snapshot for one item right now (no queue; for the CLI).
    /// The snapshot outcome is recorded on the item either way; the returned error, if
    /// any, is the metadata fetch's. Local URLs are skipped (see [`Stow::capture`]) unless `allow_local_urls`.
    pub async fn fetch_item(&self, item_id: &str) -> Result<()> {
        let it = items::get(&self.db, item_id)?;
        if self.skip_local(&it)? {
            return Ok(());
        }
        let client = fetch::client_for(self.allow_local_urls)?;
        let hook = JobQueue::noop_hook();
        let meta = jobs::run_fetch_metadata(&self.db, &client, item_id, &hook).await;
        jobs::run_fetch_snapshot(&self.db, &client, item_id, &hook).await;
        meta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn default_data_dir_ends_in_stow() {
        if let Some(d) = default_data_dir() {
            assert!(d.ends_with("Stow"));
        }
    }

    #[test]
    fn capture_without_jobs_creates_once() {
        let dir = tempfile::tempdir().unwrap();
        let stow = Stow::open(dir.path()).unwrap();
        let req = || CaptureRequest {
            url: "https://example.com/x?utm_source=y".into(),
            ..Default::default()
        };
        let a = stow.capture(req()).unwrap();
        assert!(a.created);
        let b = stow.capture(req()).unwrap();
        assert!(!b.created);
        assert_eq!(a.value.id, b.value.id);
        assert!(stow.jobs().is_none());
        assert!(matches!(
            stow.capture(CaptureRequest {
                url: "chrome://x".into(),
                ..Default::default()
            }),
            Err(StowError::InvalidUrl { .. })
        ));
    }

    #[tokio::test]
    async fn capture_with_jobs_enqueues_only_for_new_items_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = events.clone();
        let hook: jobs::JobHook = Arc::new(move |e| ev.lock().unwrap().push(e));
        let (queue, mut rx) = JobQueue::for_tests(hook);
        let stow = Stow::with_jobs(db, queue);

        let first = stow
            .capture(CaptureRequest {
                url: "https://example.com/a".into(),
                title: Some("A".into()),
                bucket_id: None,
            })
            .unwrap();
        let id = first.value.id.clone();
        assert_eq!(first.value.title.as_deref(), Some("A"));
        stow.capture(CaptureRequest {
            url: "https://example.com/a".into(),
            ..Default::default()
        })
        .unwrap();

        let mut queued = Vec::new();
        while let Ok(j) = rx.try_recv() {
            queued.push(j);
        }
        assert_eq!(
            queued,
            vec![
                Job::FetchMetadata {
                    item_id: id.clone()
                },
                Job::FetchSnapshot {
                    item_id: id.clone()
                }
            ],
            "jobs only for the new item"
        );
        assert_eq!(
            *events.lock().unwrap(),
            vec![JobEvent::ItemAdded(id.clone()), JobEvent::ItemUpdated(id)]
        );
    }

    #[tokio::test]
    async fn capture_marks_snapshot_pending() {
        use crate::models::JobStatus;
        // A quit before the jobs run must not lose the fetch: the row says `pending` from
        // the moment the jobs are queued, so `resume_pending` picks it up next launch.
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        let (queue, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        let stow = Stow::with_jobs(db.clone(), queue);

        let page = stow
            .capture(CaptureRequest {
                url: "https://example.com/article".into(),
                ..Default::default()
            })
            .unwrap();
        assert!(page.created);
        assert_eq!(
            page.value.snapshot_status,
            JobStatus::Pending,
            "returned row is fresh"
        );
        assert_eq!(
            items::get(&db, &page.value.id).unwrap().snapshot_status,
            JobStatus::Pending
        );
        let mut queued = Vec::new();
        while let Ok(j) = rx.try_recv() {
            queued.push(j);
        }
        assert_eq!(
            queued,
            vec![
                Job::FetchMetadata {
                    item_id: page.value.id.clone()
                },
                Job::FetchSnapshot {
                    item_id: page.value.id.clone()
                }
            ]
        );

        // Videos never get a snapshot: no pending mark, metadata only.
        let video = stow
            .capture(CaptureRequest {
                url: "https://youtu.be/abc123".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(video.value.snapshot_status, JobStatus::None);
        let mut queued = Vec::new();
        while let Ok(j) = rx.try_recv() {
            queued.push(j);
        }
        assert_eq!(
            queued,
            vec![Job::FetchMetadata {
                item_id: video.value.id.clone()
            }]
        );

        // Re-capturing an existing item changes nothing.
        stow.capture(CaptureRequest {
            url: "https://example.com/article".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(rx.try_recv().is_err());

        // Without a queue (the CLI fetches inline) the row is left alone.
        let stow = Stow::open(dir.path()).unwrap();
        let c = stow
            .capture(CaptureRequest {
                url: "https://example.com/cli".into(),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(c.value.snapshot_status, JobStatus::None);
    }

    #[tokio::test]
    async fn capture_of_localhost_url_is_saved_but_not_fetched() {
        use crate::models::JobStatus;
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        let (queue, mut rx) = JobQueue::for_tests(JobQueue::noop_hook());
        assert!(!queue.allow_local_urls(), "local fetching is opt-in");
        let stow = Stow::with_jobs(db.clone(), queue);
        for url in [
            "http://localhost:3000/admin",
            "http://127.0.0.1:9/x",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/",
        ] {
            let c = stow
                .capture(CaptureRequest {
                    url: url.into(),
                    ..Default::default()
                })
                .unwrap();
            assert!(c.created, "{url} saved");
            assert_eq!(
                c.value.snapshot_status,
                JobStatus::Failed,
                "returned row is fresh"
            );
            let it = items::get(&db, &c.value.id).unwrap();
            assert_eq!(it.snapshot_status, JobStatus::Failed, "{url}");
            assert_eq!(
                it.snapshot_error.as_deref(),
                Some("local address not fetched"),
                "{url}"
            );
        }
        assert!(rx.try_recv().is_err(), "no fetch jobs for local addresses");

        // fetch_item (the CLI path) skips them too, without touching the network.
        let stow = Stow::open(dir.path()).unwrap();
        let c = stow
            .capture(CaptureRequest {
                url: "http://127.0.0.1:9/other".into(),
                ..Default::default()
            })
            .unwrap();
        stow.fetch_item(&c.value.id).await.unwrap();
        let it = items::get(stow.db(), &c.value.id).unwrap();
        assert_eq!(
            it.snapshot_error.as_deref(),
            Some("local address not fetched")
        );

        // Opted in: jobs are queued as usual.
        let (queue, mut rx) = JobQueue::for_tests_with(
            JobQueue::noop_hook(),
            jobs::JobQueueOptions {
                allow_local_urls: true,
            },
        );
        let stow = Stow::with_jobs(db, queue);
        assert!(stow.allow_local_urls());
        stow.capture(CaptureRequest {
            url: "http://localhost:3000/ok".into(),
            ..Default::default()
        })
        .unwrap();
        assert!(matches!(rx.try_recv(), Ok(Job::FetchMetadata { .. })));
    }
}
