//! The SQLite database: open, migrate, and serialized access to one connection.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::IoContext;
use crate::{Result, StowError};

/// Append-only. `PRAGMA user_version` holds the number of migrations applied.
const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_v2.sql"),
];

/// Folders created under the data dir.
pub const SUBDIRS: &[&str] = &["snapshots", "attachments", "audio", "models", "assets"];

/// One SQLite connection behind a mutex. Stow is a single-user desktop app.
pub struct Db {
    conn: Mutex<Connection>,
    data_dir: PathBuf,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

impl Db {
    /// Create `data_dir` (mode 0700 on unix) and its subfolders, open `stow.db`, and
    /// apply pending migrations.
    pub fn open(data_dir: &Path) -> Result<Db> {
        Self::open_at(data_dir, MIGRATIONS.len())
    }

    /// `open`, applying only the first `upto` migrations. Tests use it to build old schemas.
    pub(crate) fn open_at(data_dir: &Path, upto: usize) -> Result<Db> {
        create_private_dir(data_dir)?;
        for sub in SUBDIRS {
            let p = data_dir.join(sub);
            std::fs::create_dir_all(&p).io_ctx("create dir", &p)?;
        }
        let mut conn = Connection::open(data_dir.join("stow.db"))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000; \
             PRAGMA synchronous = NORMAL; PRAGMA temp_store = MEMORY;",
        )?;
        migrate(&mut conn, upto)?;
        Ok(Db {
            conn: Mutex::new(conn),
            data_dir: data_dir.to_path_buf(),
        })
    }

    /// The directory holding `stow.db` and the file folders.
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Run `f` with the connection (autocommit). A poisoned lock is recovered: SQLite
    /// rolls back any transaction the panicking holder left open.
    pub(crate) fn with<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        let conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        f(&conn)
    }

    /// Run `f` inside one `BEGIN IMMEDIATE` transaction; committed only when `f` returns
    /// `Ok`. Do not call other `Db` methods from inside `f` (the lock is held).
    pub fn tx<T>(&self, f: impl FnOnce(&Transaction<'_>) -> Result<T>) -> Result<T> {
        let mut conn = self.conn.lock().unwrap_or_else(PoisonError::into_inner);
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Run blocking database work on tokio's blocking pool.
    pub async fn call<T: Send + 'static>(
        self: &Arc<Self>,
        f: impl FnOnce(&Db) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let db = Arc::clone(self);
        tokio::task::spawn_blocking(move || f(&db))
            .await
            .map_err(|e| StowError::Internal(format!("database task failed: {e}")))?
    }
}

fn create_private_dir(dir: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
            .io_ctx("create dir", dir)
    }
    #[cfg(not(unix))]
    {
        std::fs::create_dir_all(dir).io_ctx("create dir", dir)
    }
}

/// Bring `user_version` up to `upto`, one `BEGIN IMMEDIATE` transaction per migration.
/// The version is re-read inside each transaction: another process (the app and `stw`
/// opening the same dir) may have applied the step while this one waited for the lock.
fn migrate(conn: &mut Connection, upto: usize) -> Result<()> {
    let supported = i64::try_from(MIGRATIONS.len()).unwrap_or(i64::MAX);
    let version = current_version(conn)?;
    if version > supported {
        return Err(StowError::SchemaTooNew {
            found: version,
            supported,
        });
    }
    for (n, sql) in (1_i64..).zip(MIGRATIONS.iter()).take(upto) {
        if n <= version {
            continue;
        }
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if n <= current_version(&tx)? {
            continue; // applied by another process; the dropped tx rolls back nothing
        }
        tx.execute_batch(sql)?;
        if n == 2 {
            normalize_reminder_times(&tx)?;
        }
        tx.execute_batch(&format!("PRAGMA user_version = {n}"))?;
        tx.commit()?;
    }
    Ok(())
}

/// `PRAGMA user_version`, falling back to [`legacy_version`] when it was never set.
fn current_version(conn: &Connection) -> Result<i64> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == 0 {
        return legacy_version(conn);
    }
    Ok(version)
}

/// Databases created before `user_version` tracking recorded versions in `schema_migrations`.
fn legacy_version(conn: &Connection) -> Result<i64> {
    let has_table: bool = conn.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        [],
        |r| r.get(0),
    )?;
    if !has_table {
        return Ok(0);
    }
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |r| r.get(0),
    )?)
}

/// v1 stored reminder times verbatim; rewrite them in canonical UTC form so string
/// comparison against `ids::now()` orders correctly.
fn normalize_reminder_times(tx: &Transaction<'_>) -> Result<()> {
    let rows: Vec<(String, String, Option<String>, Option<String>)> = {
        let mut st = tx.prepare("SELECT id, fire_at, snoozed_until, fired_at FROM reminders")?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    let norm = |s: &str| crate::ids::parse_rfc3339(s).unwrap_or_else(|_| s.to_string());
    let mut up = tx.prepare(
        "UPDATE reminders SET fire_at = ?2, snoozed_until = ?3, fired_at = ?4 WHERE id = ?1",
    )?;
    for (id, fire_at, snoozed, fired) in rows {
        up.execute(rusqlite::params![
            id,
            norm(&fire_at),
            snoozed.as_deref().map(norm),
            fired.as_deref().map(norm)
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(db: &Db, kind: &str) -> Vec<String> {
        db.with(|c| {
            let mut st =
                c.prepare("SELECT name FROM sqlite_master WHERE type = ?1 ORDER BY name")?;
            let names = st
                .query_map([kind], |r| r.get::<_, String>(0))?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(names)
        })
        .unwrap()
    }

    fn user_version(db: &Db) -> i64 {
        db.with(|c| Ok(c.query_row("PRAGMA user_version", [], |r| r.get(0))?))
            .unwrap()
    }

    #[test]
    fn open_creates_dirs_and_tables() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Stow");
        let db = Db::open(&root).unwrap();
        for sub in ["snapshots", "attachments", "audio", "models", "assets"] {
            assert!(root.join(sub).is_dir(), "missing dir {sub}");
        }
        assert!(root.join("stow.db").is_file());
        let names = names(&db, "table");
        for t in [
            "buckets",
            "items",
            "tasks",
            "reminders",
            "attachments",
            "recordings",
            "ai_runs",
            "settings",
            "search",
            "search_docs",
        ] {
            assert!(
                names.iter().any(|n| n == t),
                "missing table {t}; have {names:?}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn data_dir_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Stow");
        Db::open(&root).unwrap();
        let mode = std::fs::metadata(&root).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn open_twice_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let a = Db::open(dir.path()).unwrap();
        drop(a);
        let b = Db::open(dir.path()).unwrap();
        assert_eq!(user_version(&b), i64::try_from(MIGRATIONS.len()).unwrap());
        assert_eq!(b.data_dir(), dir.path());
    }

    #[test]
    fn schema_too_new_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        db.with(|c| Ok(c.execute_batch("PRAGMA user_version = 99")?))
            .unwrap();
        drop(db);
        let err = Db::open(dir.path()).unwrap_err();
        assert!(
            matches!(err, StowError::SchemaTooNew { found: 99, supported } if supported == 2),
            "{err:?}"
        );
        assert_eq!(err.code(), "schema");
    }

    #[test]
    fn legacy_schema_migrations_table_counts_as_applied() {
        // A v0.1 database: 0001 applied and recorded only in schema_migrations.
        let dir = tempfile::tempdir().unwrap();
        {
            let c = Connection::open(dir.path().join("stow.db")).unwrap();
            c.execute_batch(MIGRATIONS[0]).unwrap();
            c.execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
                 INSERT INTO schema_migrations VALUES (1, '2026-01-01T00:00:00Z');",
            )
            .unwrap();
        }
        let db = Db::open(dir.path()).unwrap();
        assert_eq!(user_version(&db), 2);
        assert!(names(&db, "table").iter().any(|n| n == "search_docs"));
    }

    #[test]
    fn failing_tx_leaves_no_partial_rows() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        let r: Result<()> = db.tx(|t| {
            t.execute("INSERT INTO settings (key, value) VALUES ('a', '1')", [])?;
            t.execute("INSERT INTO settings (key, value) VALUES ('b', '2')", [])?;
            Err(StowError::Internal("injected".into()))
        });
        assert!(r.is_err());
        let n: i64 = db
            .with(|c| Ok(c.query_row("SELECT count(*) FROM settings", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0, "rolled back");
    }

    #[test]
    fn poisoned_lock_is_recovered() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        let d = db.clone();
        let _ = std::thread::spawn(move || {
            d.with(|_| -> Result<()> { panic!("poison") }).ok();
        })
        .join();
        assert!(db
            .with(|c| Ok(c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?))
            .is_ok());
    }

    #[tokio::test]
    async fn call_runs_on_blocking_pool() {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(Db::open(dir.path()).unwrap());
        let v = db
            .call(|db| db.with(|c| Ok(c.query_row("SELECT 41 + 1", [], |r| r.get::<_, i64>(0))?)))
            .await;
        assert_eq!(v.unwrap(), 42);
    }

    #[test]
    fn migration_0002_applies_on_a_v1_database() {
        let dir = tempfile::tempdir().unwrap();
        let v1 = Db::open_at(dir.path(), 1).unwrap();
        assert_eq!(user_version(&v1), 1);
        v1.with(|c| {
            c.execute_batch(
                "INSERT INTO buckets (id, name, position, created_at, updated_at)
                   VALUES ('B1', 'Blog', 1, 't', 't');
                 INSERT INTO items (id, bucket_id, url, canonical_url, title, kind, created_at, updated_at)
                   VALUES ('I1', 'B1', 'https://e.com', 'https://e.com/', 'Zebra guide', 'article', 't', 't');
                 INSERT INTO recordings (id, parent_type, parent_id, audio_path, duration_ms, created_at)
                   VALUES ('R1', 'item', 'I1', '/x.wav', 1, 't');
                 INSERT INTO reminders (id, target_type, target_id, fire_at, created_at)
                   VALUES ('M1', 'item', 'I1', '2026-01-01T07:00:00-05:00', 't');
                 INSERT INTO search (doc_type, doc_id, item_id, bucket_id, title, body)
                   VALUES ('item', 'I1', 'I1', 'B1', 'Zebra guide', 'stripes');
                 INSERT INTO search (doc_type, doc_id, item_id, bucket_id, title, body)
                   VALUES ('recording', 'R1', 'I1', NULL, 'Recording', 'quokka talk');",
            )?;
            Ok(())
        })
        .unwrap();
        drop(v1);

        let db = Db::open(dir.path()).unwrap();
        assert_eq!(user_version(&db), 2);
        let (done_at, attempts): (Option<String>, i64) = db
            .with(|c| {
                Ok(c.query_row(
                    "SELECT done_at, snapshot_attempts FROM items WHERE id = 'I1'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap();
        assert_eq!((done_at, attempts), (None, 0));
        let indexes = names(&db, "index");
        for ix in [
            "items_bucket_created",
            "items_snapshot_status",
            "reminders_unfired_effective",
            "search_docs_item",
            "search_docs_bucket",
        ] {
            assert!(
                indexes.iter().any(|n| n == ix),
                "missing index {ix}: {indexes:?}"
            );
        }
        assert!(
            !indexes.iter().any(|n| n == "items_bucket_position"),
            "old index dropped"
        );
        let triggers = names(&db, "trigger");
        for t in [
            "items_cascade",
            "buckets_cascade",
            "tasks_cascade",
            "search_docs_ai",
        ] {
            assert!(triggers.iter().any(|n| n == t), "missing trigger {t}");
        }

        let hits = crate::repo::search::query(&db, "zebra", 10).unwrap();
        assert_eq!(hits.len(), 1, "existing docs migrated and indexed");
        assert_eq!(hits[0].bucket_id.as_deref(), Some("B1"));
        let hits = crate::repo::search::query(&db, "quokka", 10).unwrap();
        assert_eq!(
            hits[0].bucket_id.as_deref(),
            Some("B1"),
            "item recording carries bucket"
        );

        let fire_at: String = db
            .with(|c| {
                Ok(
                    c.query_row("SELECT fire_at FROM reminders WHERE id = 'M1'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(
            fire_at, "2026-01-01T12:00:00.000000Z",
            "reminder times normalized"
        );
    }

    /// What one writer thread left behind, for the final reconciliation.
    #[derive(Default)]
    struct WriterOutcome {
        surviving_items: Vec<String>,
        surviving_snapshot_docs: Vec<String>,
        buckets: usize,
        search_hits_seen: usize,
    }

    #[test]
    #[allow(clippy::too_many_lines)] // one scenario; splitting it would hide the bookkeeping
    fn concurrent_cli_and_app_writers_do_not_corrupt() {
        // The app and a `stw` process each hold their own connection to one data dir and
        // interleave 200 operations each under WAL: creates, note edits (re-index),
        // snapshot indexing, deletes (cascade), bucket creates, and reads. busy_timeout
        // must absorb every SQLITE_BUSY; any "database is locked" error panics the thread.
        use crate::repo::{buckets, items, search};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_path_buf();
        Db::open(&path).unwrap();
        let writer = |tag: &'static str| {
            let path = path.clone();
            std::thread::spawn(move || -> WriterOutcome {
                let db = Db::open(&path).unwrap();
                let mode: String = db
                    .with(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?))
                    .unwrap();
                assert_eq!(mode, "wal", "{tag}");
                let mut out = WriterOutcome::default();
                let mut created: Vec<String> = Vec::new();
                let mut snapshots: Vec<String> = Vec::new();
                for i in 0..200 {
                    let it = items::create(
                        &db,
                        items::NewItem {
                            url: format!("https://example.com/{tag}/{i}"),
                            bucket_id: None,
                            title: Some(format!("{tag} {i}")),
                        },
                    )
                    .unwrap_or_else(|e| panic!("{tag} create {i}: {e}"))
                    .value;
                    items::set_notes(&db, &it.id, &format!("shared note {tag} {i}"))
                        .unwrap_or_else(|e| panic!("{tag} notes {i}: {e}"));
                    if i % 4 == 0 {
                        search::index(
                            &db,
                            "snapshot",
                            &it.id,
                            Some(&it.id),
                            None,
                            &format!("Snapshot {tag} {i}"),
                            &format!("snaptext {tag} {i}"),
                        )
                        .unwrap_or_else(|e| panic!("{tag} index {i}: {e}"));
                        snapshots.push(it.id.clone());
                    }
                    created.push(it.id);
                    if i % 5 == 4 {
                        // Delete the item created two steps ago (it may own a snapshot doc).
                        let victim = created.remove(created.len() - 3);
                        items::delete(&db, &victim)
                            .unwrap_or_else(|e| panic!("{tag} delete {i}: {e}"));
                        snapshots.retain(|s| s != &victim);
                    }
                    if i % 10 == 0 {
                        buckets::create(&db, &format!("{tag} bucket {i}"), None)
                            .unwrap_or_else(|e| panic!("{tag} bucket {i}: {e}"));
                        out.buckets += 1;
                    }
                    if i % 20 == 19 {
                        let hits = search::query(&db, "shared", 1000)
                            .unwrap_or_else(|e| panic!("{tag} search {i}: {e}"));
                        out.search_hits_seen += hits.len();
                    }
                }
                out.surviving_items = created;
                out.surviving_snapshot_docs = snapshots;
                out
            })
        };
        let (a, b) = (writer("app"), writer("cli"));
        let (a, b) = (a.join().unwrap(), b.join().unwrap());
        assert_eq!(a.surviving_items.len(), 160);
        assert_eq!(b.surviving_items.len(), 160);
        assert!(a.search_hits_seen > 0 && b.search_hits_seen > 0);

        let db = Db::open(&path).unwrap();
        let (items_n, buckets_n, item_docs, snap_docs, total_docs, check, mode): (
            i64,
            i64,
            i64,
            i64,
            i64,
            String,
            String,
        ) = db
            .with(|c| {
                let count = |sql: &str| c.query_row(sql, [], |r| r.get::<_, i64>(0));
                Ok((
                    count("SELECT count(*) FROM items")?,
                    count("SELECT count(*) FROM buckets")?,
                    count("SELECT count(*) FROM search_docs WHERE doc_type = 'item'")?,
                    count("SELECT count(*) FROM search_docs WHERE doc_type = 'snapshot'")?,
                    count("SELECT count(*) FROM search_docs")?,
                    c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?,
                    c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?,
                ))
            })
            .unwrap();
        assert_eq!(check, "ok");
        assert_eq!(mode, "wal");
        assert_eq!(items_n, 320, "160 survivors per writer");
        assert_eq!(buckets_n, i64::try_from(a.buckets + b.buckets).unwrap());
        assert_eq!(item_docs, 320, "one item doc per surviving item");
        let expected_snaps = a.surviving_snapshot_docs.len() + b.surviving_snapshot_docs.len();
        assert_eq!(snap_docs, i64::try_from(expected_snaps).unwrap());
        assert_eq!(
            total_docs,
            320 + i64::try_from(expected_snaps).unwrap() + buckets_n,
            "no orphan search docs"
        );
        // Every survivor is present and findable; every victim is gone from both tables.
        for id in a.surviving_items.iter().chain(&b.surviving_items) {
            items::get(&db, id).unwrap_or_else(|e| panic!("{id}: {e}"));
        }
        let hits = search::query(&db, "shared", 1000).unwrap();
        assert_eq!(hits.len(), 320);
        assert_eq!(
            search::query(&db, "snaptext", 1000).unwrap().len(),
            expected_snaps
        );
        assert_eq!(
            search::query(&db, "cli", 1000).unwrap().len(),
            160 + b.surviving_snapshot_docs.len() + b.buckets,
            "cli items + cli snapshot docs + cli buckets"
        );
        assert_eq!(
            items::count(&db, &items::ItemFilter::Inbox, true).unwrap(),
            320
        );
        // The FTS index agrees with its content table (external-content FTS can drift).
        db.with(|c| {
            c.execute("INSERT INTO search (search) VALUES ('integrity-check')", [])?;
            Ok(())
        })
        .expect("fts integrity-check");
    }

    #[test]
    #[allow(clippy::too_many_lines)] // every table is seeded and checked in one place
    fn migration_0002_preserves_rows_in_every_table_from_a_v1_database() {
        // Apply only 0001, fill every table (search in its OLD plain-fts5 layout), then
        // open normally: 0002 must run, keep every row, and index the old docs.
        let dir = tempfile::tempdir().unwrap();
        let v1 = Db::open_at(dir.path(), 1).unwrap();
        assert_eq!(user_version(&v1), 1);
        let old_tables = names(&v1, "table");
        assert!(
            !old_tables.iter().any(|n| n == "search_docs"),
            "v1 has no search_docs"
        );
        v1.with(|c| {
            c.execute_batch(
                "INSERT INTO buckets (id, name, emoji, description_md, notes_md, position, created_at, updated_at)
                   VALUES ('B1', 'Blog', '✍️', 'about writing', 'bucket notes okapi', 1, 't', 't'),
                          ('B2', 'Archived', NULL, '', '', 2, 't', 't');
                 UPDATE buckets SET archived_at = 't' WHERE id = 'B2';
                 INSERT INTO items (id, bucket_id, url, canonical_url, title, description, site_name, kind,
                                    favicon_path, image_path, notes_md, snapshot_status, snapshot_error,
                                    snapshot_fetched_at, position, created_at, updated_at)
                   VALUES ('I1', 'B1', 'https://e.com/a?utm_source=x', 'https://e.com/a', 'Zebra guide', 'desc',
                           'E', 'article', '/f.png', '/i.jpg', 'item notes', 'done', NULL, 't', 1, 't', 't'),
                          ('I2', NULL, 'https://e.com/b', 'https://e.com/b', 'Inbox thing', NULL, NULL, 'video',
                           NULL, NULL, '', 'pending', NULL, NULL, 2, 't', 't'),
                          ('I3', 'B2', 'https://e.com/c', 'https://e.com/c', 'Failed one', NULL, NULL, 'other',
                           NULL, NULL, '', 'failed', 'boom', NULL, 3, 't', 't');
                 INSERT INTO tasks (id, parent_type, parent_id, title, done_at, position, created_at)
                   VALUES ('T1', 'item', 'I1', 'read it', NULL, 1, 't'),
                          ('T2', 'bucket', 'B1', 'plan it', 't', 1, 't');
                 INSERT INTO reminders (id, target_type, target_id, fire_at, message, fired_at, snoozed_until, created_at)
                   VALUES ('M1', 'item', 'I1', '2026-01-01T07:00:00-05:00', 'msg', NULL, NULL, 't'),
                          ('M2', 'task', 'T1', '2026-02-01T00:00:00Z', NULL, '2026-02-01T00:00:01Z', '2026-02-02T09:00:00+09:00', 't'),
                          ('M3', 'bucket', 'B1', '2026-03-01T00:00:00Z', NULL, NULL, NULL, 't');
                 INSERT INTO attachments (id, parent_type, parent_id, filename, path, mime, size_bytes, created_at)
                   VALUES ('A1', 'item', 'I1', 'diagram.png', '/att/A1/diagram.png', 'image/png', 3, 't'),
                          ('A2', 'bucket', 'B1', 'brief.pdf', '/att/A2/brief.pdf', 'application/pdf', 9, 't');
                 INSERT INTO recordings (id, parent_type, parent_id, audio_path, duration_ms, transcript_md,
                                         transcript_status, transcript_error, created_at)
                   VALUES ('R1', 'item', 'I1', '/x.wav', 1000, '**[00:00]** quokka talk', 'done', NULL, 't'),
                          ('R2', 'bucket', 'B1', '/y.wav', 500, NULL, 'pending', NULL, 't'),
                          ('R3', 'item', 'I2', '/z.wav', 500, NULL, 'running', NULL, 't');
                 INSERT INTO ai_runs (id, parent_type, parent_id, action, provider, model, input_hash, output_md,
                                      status, error, created_at)
                   VALUES ('X1', 'item', 'I1', 'summarize_page', 'anthropic', 'm', 'h1', 'axolotl summary', 'done', NULL, 't'),
                          ('X2', 'bucket', 'B1', 'suggest_tasks', 'openai', 'm', 'h2', NULL, 'failed', 'boom', 't'),
                          ('X3', 'item', 'I2', 'summarize_notes', 'openai', 'm', 'h3', NULL, 'running', NULL, 't');
                 INSERT INTO settings (key, value)
                   VALUES ('theme', 'dark'), ('whisper_model', 'tiny.en'), ('extension_token', 'secret');
                 INSERT INTO search (doc_type, doc_id, item_id, bucket_id, title, body)
                   VALUES ('bucket', 'B1', NULL, 'B1', 'Blog', 'about writing\nbucket notes okapi'),
                          ('item', 'I1', 'I1', 'B1', 'Zebra guide', 'desc\nitem notes\nhttps://e.com/a'),
                          ('item', 'I2', 'I2', NULL, 'Inbox thing', 'https://e.com/b'),
                          ('item', 'I3', 'I3', 'B2', 'Failed one', 'https://e.com/c'),
                          ('snapshot', 'I1', 'I1', 'B1', 'Zebra guide', 'stripes on the savannah'),
                          ('recording', 'R1', 'I1', NULL, 'Recording', 'quokka talk'),
                          ('ai_run', 'X1', 'I1', NULL, 'Summary', 'axolotl summary');",
            )?;
            Ok(())
        })
        .unwrap();
        let count_v1 = |table: &str| -> i64 {
            v1.with(
                |c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?),
            )
            .unwrap()
        };
        let before: Vec<(&str, i64)> = [
            "buckets",
            "items",
            "tasks",
            "reminders",
            "attachments",
            "recordings",
            "ai_runs",
            "settings",
        ]
        .into_iter()
        .map(|t| (t, count_v1(t)))
        .collect();
        assert_eq!(count_v1("search"), 7);
        drop(v1);

        let db = Db::open(dir.path()).unwrap();
        assert_eq!(user_version(&db), 2);
        let count = |table: &str| -> i64 {
            db.with(
                |c| Ok(c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?),
            )
            .unwrap()
        };
        for (table, n) in &before {
            assert_eq!(count(table), *n, "{table} lost rows");
        }
        assert_eq!(count("search_docs"), 7, "every old search row migrated");
        let (check, fk): (String, i64) = db
            .with(|c| {
                Ok((
                    c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?,
                    c.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                        r.get(0)
                    })?,
                ))
            })
            .unwrap();
        assert_eq!(check, "ok");
        assert_eq!(fk, 0);

        // Row contents survive, with the new columns at their defaults.
        let i1 = crate::repo::items::get(&db, "I1").unwrap();
        assert_eq!(i1.title.as_deref(), Some("Zebra guide"));
        assert_eq!(i1.description.as_deref(), Some("desc"));
        assert_eq!(i1.notes_md, "item notes");
        assert_eq!(i1.bucket_id.as_deref(), Some("B1"));
        assert_eq!(i1.snapshot_status, crate::models::JobStatus::Done);
        assert_eq!(i1.done_at, None);
        let i3 = crate::repo::items::get(&db, "I3").unwrap();
        assert_eq!(i3.snapshot_error.as_deref(), Some("boom"));
        let b1 = crate::repo::buckets::get(&db, "B1").unwrap();
        assert_eq!(b1.emoji.as_deref(), Some("✍️"));
        assert_eq!(b1.notes_md, "bucket notes okapi");
        assert!(crate::repo::buckets::get(&db, "B2")
            .unwrap()
            .archived_at
            .is_some());
        assert_eq!(
            crate::repo::tasks::get(&db, "T2")
                .unwrap()
                .done_at
                .as_deref(),
            Some("t")
        );
        let r1 = crate::repo::recordings::get(&db, "R1").unwrap();
        assert_eq!(r1.transcript_md.as_deref(), Some("**[00:00]** quokka talk"));
        let (ra, ia): (i64, i64) = db
            .with(|c| {
                Ok((
                    c.query_row(
                        "SELECT transcript_attempts FROM recordings WHERE id = 'R3'",
                        [],
                        |r| r.get(0),
                    )?,
                    c.query_row(
                        "SELECT snapshot_attempts FROM items WHERE id = 'I2'",
                        [],
                        |r| r.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!((ra, ia), (0, 0), "new counters start at 0");
        let x1 = crate::repo::ai_runs::get(&db, "X1").unwrap();
        assert_eq!(x1.output_md.as_deref(), Some("axolotl summary"));
        assert_eq!(
            crate::repo::attachments::get(&db, "A2").unwrap().filename,
            "brief.pdf"
        );
        assert_eq!(
            crate::repo::settings::get(&db, "extension_token")
                .unwrap()
                .as_deref(),
            Some("secret")
        );
        // Reminder times were normalized; the snoozed one kept its snooze.
        let m2 = crate::repo::reminders::get(&db, "M2").unwrap();
        assert_eq!(m2.fire_at, "2026-02-01T00:00:00.000000Z");
        assert_eq!(m2.fired_at.as_deref(), Some("2026-02-01T00:00:01.000000Z"));
        assert_eq!(
            m2.snoozed_until.as_deref(),
            Some("2026-02-02T00:00:00.000000Z")
        );

        // Search finds every old doc, by a word unique to each.
        let hit = |word: &str| {
            let hits = crate::repo::search::query(&db, word, 10).unwrap();
            assert_eq!(hits.len(), 1, "{word}: {hits:?}");
            hits.into_iter().next().unwrap()
        };
        assert_eq!(hit("okapi").doc_type, "bucket");
        assert_eq!(hit("okapi").bucket_id.as_deref(), Some("B1"));
        assert_eq!(hit("savannah").doc_type, "snapshot");
        assert_eq!(hit("quokka").doc_type, "recording");
        assert_eq!(
            hit("quokka").bucket_id.as_deref(),
            Some("B1"),
            "item-owned docs gain the item's bucket"
        );
        assert_eq!(hit("axolotl").doc_type, "ai_run");
        assert_eq!(hit("axolotl").bucket_id.as_deref(), Some("B1"));
        assert_eq!(hit("inbox").doc_id, "I2");
        assert_eq!(hit("inbox").bucket_id, None);
        assert_eq!(hit("failed").bucket_id.as_deref(), Some("B2"));
        assert_eq!(
            crate::repo::search::query(&db, "zebra", 10).unwrap().len(),
            2,
            "item doc and snapshot doc"
        );

        // The migrated tables behave: cascades and re-indexing work on old rows.
        crate::repo::items::delete(&db, "I1").unwrap();
        assert_eq!(count("items"), 2);
        assert_eq!(count("tasks"), 1, "item task cascaded");
        assert_eq!(count("reminders"), 1, "item + task reminders cascaded");
        assert_eq!(count("attachments"), 1);
        assert_eq!(count("recordings"), 2);
        assert_eq!(count("ai_runs"), 2);
        assert_eq!(
            count("search_docs"),
            3,
            "item, snapshot, recording, ai_run docs gone"
        );
        assert!(crate::repo::search::query(&db, "quokka", 10)
            .unwrap()
            .is_empty());
        assert_eq!(hit("okapi").doc_type, "bucket");
    }

    #[test]
    fn migration_chain_applies_from_empty_and_from_v1() {
        let latest = i64::try_from(MIGRATIONS.len()).unwrap();
        // From nothing: every migration in order.
        let dir = tempfile::tempdir().unwrap();
        let fresh = Db::open(dir.path()).unwrap();
        assert_eq!(user_version(&fresh), latest);
        let fresh_tables = names(&fresh, "table");
        let fresh_indexes = names(&fresh, "index");
        let fresh_triggers = names(&fresh, "trigger");
        // From v1: the remaining migrations produce the same schema objects.
        let dir = tempfile::tempdir().unwrap();
        drop(Db::open_at(dir.path(), 1).unwrap());
        let upgraded = Db::open(dir.path()).unwrap();
        assert_eq!(user_version(&upgraded), latest);
        assert_eq!(names(&upgraded, "table"), fresh_tables);
        assert_eq!(names(&upgraded, "index"), fresh_indexes);
        assert_eq!(names(&upgraded, "trigger"), fresh_triggers);
    }

    #[test]
    fn concurrent_migrations_from_v1_do_not_collide() {
        // The app and `stw` both open an old data dir at the same moment. Migration 0002
        // is not idempotent (ALTER TABLE ADD COLUMN), so the second opener must see the
        // first one's work inside its own write transaction and skip it.
        use std::sync::{Arc, Barrier};
        let dir = tempfile::tempdir().unwrap();
        drop(Db::open_at(dir.path(), 1).unwrap());
        let gate = Arc::new(Barrier::new(2));
        let openers: Vec<_> = (0..2)
            .map(|_| {
                let (path, gate) = (dir.path().to_path_buf(), gate.clone());
                std::thread::spawn(move || {
                    gate.wait();
                    Db::open(&path).map(|db| user_version(&db))
                })
            })
            .collect();
        for h in openers {
            let v = h
                .join()
                .unwrap()
                .expect("open while another process migrates");
            assert_eq!(v, 2);
        }
        let db = Db::open(dir.path()).unwrap();
        let (check, cols): (String, i64) = db
            .with(|c| {
                Ok((
                    c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?,
                    c.query_row(
                        "SELECT count(*) FROM pragma_table_info('items') WHERE name = 'done_at'",
                        [],
                        |r| r.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(check, "ok");
        assert_eq!(cols, 1, "migration applied exactly once");
    }
}
