//! Saved links. Every mutation bumps `updated_at` and re-indexes the `item` search doc in
//! the same transaction.

use std::path::{Path, PathBuf};

use rusqlite::{params, params_from_iter, types::Value, Connection, OptionalExtension, Row};

use crate::capture::{classify_kind, normalize_url};
use crate::error::{EntityKind, IoContext};
use crate::ids::{new_id, now};
use crate::models::{Created, Item, JobStatus, ParentType};
use crate::repo::search;
use crate::{Db, Result, StowError};

/// Input to [`create`].
#[derive(Debug, Clone, serde::Deserialize)]
pub struct NewItem {
    /// Raw URL as captured or pasted; normalized before saving.
    pub url: String,
    /// Target bucket; `None` = Inbox.
    pub bucket_id: Option<String>,
    /// Title to use instead of the host-name placeholder.
    pub title: Option<String>,
}

/// Which items to list. JSON: `{kind:'inbox'} | {kind:'bucket', id} | {kind:'all'}`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum ItemFilter {
    /// Items without a bucket.
    Inbox,
    /// Items in this bucket.
    Bucket(String),
    /// Every item.
    All,
}

/// Metadata to merge into an item. `None` fields are left untouched.
#[derive(Debug, Default, Clone)]
pub struct MetaPatch {
    /// Fetched title; only replaces the host-name placeholder.
    pub title: Option<String>,
    /// Meta/OpenGraph description.
    pub description: Option<String>,
    /// Site name.
    pub site_name: Option<String>,
    /// Local path of the downloaded favicon.
    pub favicon_path: Option<String>,
    /// Local path of the downloaded preview image.
    pub image_path: Option<String>,
}

const COLS: &str = "id, bucket_id, url, canonical_url, title, description, site_name, kind, \
    favicon_path, image_path, notes_md, snapshot_status, snapshot_error, snapshot_fetched_at, \
    done_at, position, created_at, updated_at";

fn row(r: &Row) -> rusqlite::Result<Item> {
    Ok(Item {
        id: r.get(0)?,
        bucket_id: r.get(1)?,
        url: r.get(2)?,
        canonical_url: r.get(3)?,
        title: r.get(4)?,
        description: r.get(5)?,
        site_name: r.get(6)?,
        kind: r.get(7)?,
        favicon_path: r.get(8)?,
        image_path: r.get(9)?,
        notes_md: r.get(10)?,
        snapshot_status: r.get(11)?,
        snapshot_error: r.get(12)?,
        snapshot_fetched_at: r.get(13)?,
        done_at: r.get(14)?,
        position: r.get(15)?,
        created_at: r.get(16)?,
        updated_at: r.get(17)?,
    })
}

fn not_found(id: &str) -> StowError {
    StowError::not_found(EntityKind::Item, id)
}

/// Run an `UPDATE items … WHERE id = ?1 RETURNING {COLS}` statement.
fn update_returning(c: &Connection, sql: &str, p: impl rusqlite::Params, id: &str) -> Result<Item> {
    c.prepare_cached(&format!("{sql} RETURNING {COLS}"))?
        .query_row(p, row)
        .optional()?
        .ok_or_else(|| not_found(id))
}

fn reindex_conn(c: &Connection, it: &Item) -> Result<()> {
    let body = format!(
        "{}\n{}\n{}",
        it.description.as_deref().unwrap_or(""),
        it.notes_md,
        it.url
    );
    search::index_conn(
        c,
        "item",
        &it.id,
        Some(&it.id),
        it.bucket_id.as_deref(),
        it.title.as_deref().unwrap_or(""),
        &body,
    )
}

pub(crate) fn get_conn(c: &Connection, id: &str) -> Result<Item> {
    c.prepare_cached(&format!("SELECT {COLS} FROM items WHERE id = ?1"))?
        .query_row([id], row)
        .optional()?
        .ok_or_else(|| not_found(id))
}

fn find_by_canonical_conn(c: &Connection, canonical_url: &str) -> Result<Option<Item>> {
    Ok(c.prepare_cached(&format!(
        "SELECT {COLS} FROM items WHERE canonical_url = ?1"
    ))?
    .query_row([canonical_url], row)
    .optional()?)
}

fn require_bucket(c: &Connection, bucket_id: &str) -> Result<()> {
    let exists: bool = c
        .prepare_cached("SELECT count(*) > 0 FROM buckets WHERE id = ?1")?
        .query_row([bucket_id], |r| r.get(0))?;
    if exists {
        Ok(())
    } else {
        Err(StowError::not_found(EntityKind::Bucket, bucket_id))
    }
}

/// Save a URL. Same canonical URL → returns the existing item (`created: false`),
/// moving it into `bucket_id` only when it was sitting in the Inbox. Insert, dedupe, and
/// indexing run in one transaction.
pub fn create(db: &Db, new: NewItem) -> Result<Created<Item>> {
    let url = normalize_url(&new.url)?;
    let id = new_id();
    let ts = now();
    let kind = classify_kind(&url);
    let title = new
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
        .or_else(|| url.host_str().map(str::to_string));
    db.tx(|t| {
        if let Some(b) = &new.bucket_id {
            require_bucket(t, b)?;
        }
        let inserted = t
            .prepare_cached(&format!(
                "INSERT INTO items (id, bucket_id, url, canonical_url, title, kind, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7) \
                 ON CONFLICT(canonical_url) DO NOTHING RETURNING {COLS}"
            ))?
            .query_row(params![id, new.bucket_id, new.url.trim(), url.as_str(), title, kind, ts], row)
            .optional()?;
        if let Some(it) = inserted {
            reindex_conn(t, &it)?;
            return Ok(Created { value: it, created: true });
        }
        let existing =
            find_by_canonical_conn(t, url.as_str())?.ok_or_else(|| not_found(url.as_str()))?;
        if existing.bucket_id.is_none() && new.bucket_id.is_some() {
            let moved = move_to_conn(t, &existing.id, new.bucket_id.as_deref())?;
            return Ok(Created { value: moved, created: false });
        }
        Ok(Created { value: existing, created: false })
    })
}

/// One item by id.
pub fn get(db: &Db, id: &str) -> Result<Item> {
    db.with(|c| get_conn(c, id))
}

/// The item saved under this canonical URL, if any.
pub fn find_by_canonical(db: &Db, canonical_url: &str) -> Result<Option<Item>> {
    db.with(|c| find_by_canonical_conn(c, canonical_url))
}

/// Up to 3 items whose id starts with `prefix` (case-insensitive), oldest first.
/// Enough to tell "unique", "ambiguous", and "none" apart.
pub fn find_by_prefix(db: &Db, prefix: &str) -> Result<Vec<Item>> {
    let prefix = prefix.trim().to_ascii_uppercase();
    if prefix.is_empty() {
        return Err(StowError::invalid("prefix", prefix));
    }
    let pattern = format!(
        "{}%",
        prefix
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM items WHERE id LIKE ?1 ESCAPE '\\' ORDER BY id LIMIT 3"
        ))?;
        let v = st
            .query_map([pattern], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

fn filter_sql(filter: &ItemFilter, include_done: bool) -> (String, Vec<Value>) {
    let mut args = Vec::new();
    let mut clause = match filter {
        ItemFilter::Inbox => "bucket_id IS NULL".to_string(),
        ItemFilter::Bucket(id) => {
            args.push(Value::Text(id.clone()));
            "bucket_id = ?1".to_string()
        }
        ItemFilter::All => "1".to_string(),
    };
    if !include_done {
        clause.push_str(" AND done_at IS NULL");
    }
    (clause, args)
}

/// Newest first. `include_done: false` hides items marked done.
pub fn list(db: &Db, filter: ItemFilter, include_done: bool) -> Result<Vec<Item>> {
    let (clause, args) = filter_sql(&filter, include_done);
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM items WHERE {clause} ORDER BY created_at DESC, id DESC"
        ))?;
        let v = st
            .query_map(params_from_iter(args), row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

/// How many items [`list`] would return.
pub fn count(db: &Db, filter: &ItemFilter, include_done: bool) -> Result<usize> {
    let (clause, args) = filter_sql(filter, include_done);
    db.with(|c| {
        let n: i64 = c
            .prepare_cached(&format!("SELECT count(*) FROM items WHERE {clause}"))?
            .query_row(params_from_iter(args), |r| r.get(0))?;
        Ok(usize::try_from(n).unwrap_or(0))
    })
}

/// How long a placeholder-titled item keeps getting its metadata re-fetched on launch
/// (see [`needs_metadata`]). Older ones are left alone: the site never answered.
pub const METADATA_RETRY_DAYS: i64 = 7;

/// True when the item still shows what capture guessed (no title or the host-name
/// placeholder, or no favicon) and was created within [`METADATA_RETRY_DAYS`] of `now`
/// (canonical RFC 3339), so a metadata fetch is worth queueing again.
pub fn needs_metadata(it: &Item, now: &str) -> bool {
    let recent = match (
        chrono::DateTime::parse_from_rfc3339(now),
        chrono::DateTime::parse_from_rfc3339(&it.created_at),
    ) {
        (Ok(now), Ok(created)) => {
            now.signed_duration_since(created) <= chrono::Duration::days(METADATA_RETRY_DAYS)
        }
        _ => false,
    };
    if !recent {
        return false;
    }
    let host = url::Url::parse(&it.canonical_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string));
    it.title.is_none() || it.title == host || it.favicon_path.is_none()
}

/// Rows `jobs::resume_pending` looks at: every pending snapshot, plus videos (which never
/// get a snapshot) created since `since`, oldest first.
pub(crate) fn list_resumable_conn(c: &Connection, since: &str) -> Result<Vec<Item>> {
    let mut st = c.prepare_cached(&format!(
        "SELECT {COLS} FROM items WHERE snapshot_status = 'pending' \
         OR (kind = 'video' AND created_at >= ?1) ORDER BY created_at, id"
    ))?;
    let v = st
        .query_map([since], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(v)
}

/// Items whose snapshot job was queued or in flight, oldest first. Used to re-enqueue
/// work after a restart.
pub fn list_snapshot_unfinished(db: &Db) -> Result<Vec<Item>> {
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM items WHERE snapshot_status IN ('pending', 'running') ORDER BY created_at, id"
        ))?;
        let v = st.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

fn move_to_conn(c: &Connection, id: &str, bucket_id: Option<&str>) -> Result<Item> {
    if let Some(b) = bucket_id {
        require_bucket(c, b)?;
    }
    let it = update_returning(
        c,
        "UPDATE items SET bucket_id = ?2, updated_at = ?3 WHERE id = ?1",
        params![id, bucket_id, now()],
        id,
    )?;
    search::set_item_bucket_conn(c, id, bucket_id)?;
    reindex_conn(c, &it)?;
    Ok(it)
}

/// Move into `bucket_id` (`None` = Inbox); every search doc of the item follows.
pub fn move_to(db: &Db, id: &str, bucket_id: Option<&str>) -> Result<Item> {
    db.tx(|t| move_to_conn(t, id, bucket_id))
}

/// Replace the item's Markdown notes.
pub fn set_notes(db: &Db, id: &str, notes_md: &str) -> Result<Item> {
    db.tx(|t| {
        let it = update_returning(
            t,
            "UPDATE items SET notes_md = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, notes_md, now()],
            id,
        )?;
        reindex_conn(t, &it)?;
        Ok(it)
    })
}

/// Mark done (`done_at` = now, kept if already done) or not done (`done_at` = NULL).
pub fn set_done(db: &Db, id: &str, done: bool) -> Result<Item> {
    db.with(|c| {
        update_returning(
            c,
            "UPDATE items SET done_at = CASE WHEN ?2 THEN COALESCE(done_at, ?3) ELSE NULL END, \
             updated_at = ?3 WHERE id = ?1",
            params![id, done, now()],
            id,
        )
    })
}

/// Merge fetched metadata. A fetched title only replaces the host-name placeholder,
/// never a title the user typed.
pub fn set_metadata(db: &Db, id: &str, patch: &MetaPatch) -> Result<Item> {
    db.tx(|t| {
        let cur = get_conn(t, id)?;
        let host = url::Url::parse(&cur.canonical_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string));
        let title_is_placeholder = cur.title.is_none() || cur.title == host;
        let title = match (&patch.title, title_is_placeholder) {
            (Some(t), true) if !t.trim().is_empty() => Some(t.trim().to_string()),
            _ => cur.title.clone(),
        };
        let it = update_returning(
            t,
            "UPDATE items SET title = ?2, description = COALESCE(?3, description), \
             site_name = COALESCE(?4, site_name), favicon_path = COALESCE(?5, favicon_path), \
             image_path = COALESCE(?6, image_path), updated_at = ?7 WHERE id = ?1",
            params![
                id,
                title,
                patch.description,
                patch.site_name,
                patch.favicon_path,
                patch.image_path,
                now()
            ],
            id,
        )?;
        reindex_conn(t, &it)?;
        Ok(it)
    })
}

/// Record the snapshot job's state (and error, for `Failed`).
pub fn set_snapshot_status(
    db: &Db,
    id: &str,
    status: JobStatus,
    error: Option<&str>,
) -> Result<()> {
    db.with(|c| {
        update_returning(
            c,
            "UPDATE items SET snapshot_status = ?2, snapshot_error = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, status, error, now()],
            id,
        )
        .map(drop)
    })
}

/// Mark the snapshot `Pending` the moment its jobs are queued, so a quit before they run
/// leaves a row `jobs::resume_pending` picks up. Returns the updated item.
pub fn mark_snapshot_pending(db: &Db, id: &str) -> Result<Item> {
    db.with(|c| {
        update_returning(
            c,
            "UPDATE items SET snapshot_status = 'pending', snapshot_error = NULL, updated_at = ?2 \
             WHERE id = ?1",
            params![id, now()],
            id,
        )
    })
}

/// Mark the snapshot `Running` and count the attempt (see `jobs::resume_pending`).
pub(crate) fn start_snapshot(db: &Db, id: &str) -> Result<()> {
    db.with(|c| {
        update_returning(
            c,
            "UPDATE items SET snapshot_status = 'running', snapshot_error = NULL, \
             snapshot_attempts = snapshot_attempts + 1, updated_at = ?2 WHERE id = ?1",
            params![id, now()],
            id,
        )
        .map(drop)
    })
}

/// Mark the snapshot fetched now.
pub fn set_snapshot_done(db: &Db, id: &str) -> Result<()> {
    db.with(|c| {
        update_returning(
            c,
            "UPDATE items SET snapshot_status = 'done', snapshot_error = NULL, \
             snapshot_fetched_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now()],
            id,
        )
        .map(drop)
    })
}

/// Index the readable snapshot text and mark the snapshot done, atomically.
pub(crate) fn finish_snapshot(db: &Db, id: &str, title: &str, text: &str) -> Result<()> {
    db.tx(|t| {
        let it = update_returning(
            t,
            "UPDATE items SET snapshot_status = 'done', snapshot_error = NULL, \
             snapshot_fetched_at = ?2, updated_at = ?2 WHERE id = ?1",
            params![id, now()],
            id,
        )?;
        search::index_conn(
            t,
            "snapshot",
            id,
            Some(id),
            it.bucket_id.as_deref(),
            title,
            text,
        )
    })
}

/// Files a bucket or item owns through its child rows, collected inside a transaction
/// and removed only after it commits.
#[derive(Debug, Default)]
pub(crate) struct OwnedFiles {
    dirs: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl OwnedFiles {
    pub(crate) fn collect(
        c: &Connection,
        data_dir: &Path,
        parent_type: ParentType,
        parent_id: &str,
    ) -> Result<OwnedFiles> {
        let mut out = OwnedFiles::default();
        let mut st = c.prepare_cached(
            "SELECT id FROM attachments WHERE parent_type = ?1 AND parent_id = ?2",
        )?;
        for id in st.query_map(params![parent_type, parent_id], |r| r.get::<_, String>(0))? {
            out.dirs.push(data_dir.join("attachments").join(id?));
        }
        let mut st = c.prepare_cached(
            "SELECT audio_path FROM recordings WHERE parent_type = ?1 AND parent_id = ?2",
        )?;
        for p in st.query_map(params![parent_type, parent_id], |r| r.get::<_, String>(0))? {
            out.files
                .push(crate::repo::recordings::resolve_audio_path(data_dir, &p?));
        }
        Ok(out)
    }

    /// Best effort: a missing file is fine, anything else is logged.
    pub(crate) fn remove(self) {
        for d in self.dirs {
            log_unless_missing(std::fs::remove_dir_all(&d).io_ctx("remove dir", &d));
        }
        for f in self.files {
            log_unless_missing(std::fs::remove_file(&f).io_ctx("remove", &f));
        }
    }
}

fn log_unless_missing(r: Result<()>) {
    match r {
        Err(StowError::Io { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("cleanup failed: {e}"),
        Ok(()) => {}
    }
}

/// Delete the item with everything hanging off it. Rows (tasks and their reminders, item
/// reminders, attachments, recordings, AI runs, search docs) go in one transaction via
/// the cascade triggers; files (attachment dirs, audio, snapshots, `assets/<id>-*`) are
/// removed after the commit.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    let data_dir = db.data_dir();
    let mut files = db.tx(|t| {
        get_conn(t, id)?;
        let files = OwnedFiles::collect(t, data_dir, ParentType::Item, id)?;
        t.prepare_cached("DELETE FROM items WHERE id = ?1")?
            .execute([id])?;
        Ok(files)
    })?;
    files.files.extend(own_files(data_dir, id));
    files.remove();
    Ok(())
}

/// The files an item owns directly: `snapshots/<id>.{md,html}` and `assets/<id>-*`.
fn own_files(data_dir: &Path, id: &str) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = ["md", "html"]
        .iter()
        .map(|ext| data_dir.join("snapshots").join(format!("{id}.{ext}")))
        .collect();
    let prefix = format!("{id}-");
    if let Ok(entries) = std::fs::read_dir(data_dir.join("assets")) {
        files.extend(
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(&prefix))
                .map(|e| e.path()),
        );
    }
    files
}

/// Remove the snapshot and asset files of an item whose row is already gone (a fetch
/// job that finished after the user deleted the item). Best effort, like [`delete`].
pub(crate) fn remove_own_files(data_dir: &Path, id: &str) {
    OwnedFiles {
        dirs: Vec::new(),
        files: own_files(data_dir, id),
    }
    .remove();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ItemKind, JobStatus};
    use crate::repo::{buckets, search, test_db};

    fn new(url: &str, bucket_id: Option<String>) -> NewItem {
        NewItem {
            url: url.into(),
            bucket_id,
            title: None,
        }
    }

    fn mk(db: &Db, url: &str, bucket_id: Option<String>) -> Item {
        create(db, new(url, bucket_id)).unwrap().value
    }

    #[test]
    fn item_filter_json_shapes_match_contract() {
        assert!(matches!(
            serde_json::from_str::<ItemFilter>(r#"{"kind":"inbox"}"#).unwrap(),
            ItemFilter::Inbox
        ));
        assert!(matches!(
            serde_json::from_str::<ItemFilter>(r#"{"kind":"all"}"#).unwrap(),
            ItemFilter::All
        ));
        assert!(
            matches!(serde_json::from_str::<ItemFilter>(r#"{"kind":"bucket","id":"B1"}"#).unwrap(), ItemFilter::Bucket(ref b) if b == "B1")
        );
    }

    #[test]
    fn move_to_updates_bucket_on_every_search_doc_of_the_item() {
        let (_d, db) = test_db();
        let a = buckets::create(&db, "A", None).unwrap();
        let b = buckets::create(&db, "B", None).unwrap();
        let it = mk(&db, "https://example.com/s", Some(a.id.clone()));
        search::index(
            &db,
            "snapshot",
            &it.id,
            Some(&it.id),
            Some(&a.id),
            "T",
            "giraffe body",
        )
        .unwrap();
        move_to(&db, &it.id, Some(&b.id)).unwrap();
        let hits = search::query(&db, "giraffe", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].bucket_id.as_deref(), Some(b.id.as_str()));
    }

    #[test]
    fn create_reports_whether_it_inserted() {
        let (_d, db) = test_db();
        let first = create(&db, new("https://example.com/c", None)).unwrap();
        assert!(first.created);
        let again = create(&db, new("https://example.com/c?utm_source=x", None)).unwrap();
        assert!(!again.created);
        assert_eq!(again.value.id, first.value.id);
    }

    #[test]
    fn create_into_missing_bucket_is_not_found() {
        let (_d, db) = test_db();
        let err = create(&db, new("https://example.com/m", Some("NOPE".into()))).unwrap_err();
        assert!(
            matches!(
                err,
                StowError::NotFound {
                    kind: EntityKind::Bucket,
                    ..
                }
            ),
            "{err:?}"
        );
        assert_eq!(count(&db, &ItemFilter::All, true).unwrap(), 0);
    }

    #[test]
    fn items_create_dedupes_by_canonical_and_moves_from_inbox() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Watch", None).unwrap();
        let first = mk(&db, "https://youtu.be/abc", None);
        assert_eq!(first.kind, ItemKind::Video);
        assert_eq!(first.canonical_url, "https://www.youtube.com/watch?v=abc");
        let second = mk(
            &db,
            "https://www.youtube.com/watch?v=abc&feature=share",
            Some(b.id.clone()),
        );
        assert_eq!(
            second.id, first.id,
            "same canonical url returns existing item"
        );
        assert_eq!(
            second.bucket_id.as_deref(),
            Some(b.id.as_str()),
            "moved out of inbox"
        );
        assert_eq!(list(&db, ItemFilter::All, true).unwrap().len(), 1);
    }

    #[test]
    fn items_create_is_idempotent_under_conflict() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "B", None).unwrap();
        let first = mk(&db, "https://example.com/dup?utm_source=x", None);
        let second = mk(&db, "https://example.com/dup", None);
        assert_eq!(second.id, first.id);
        assert_eq!(second.url, first.url, "original url kept");
        assert_eq!(list(&db, ItemFilter::All, true).unwrap().len(), 1);
        let third = mk(&db, "https://example.com/dup", Some(b.id.clone()));
        assert_eq!(third.id, first.id);
        assert_eq!(
            third.bucket_id.as_deref(),
            Some(b.id.as_str()),
            "inbox item moves to bucket on conflict"
        );
        assert_eq!(list(&db, ItemFilter::All, true).unwrap().len(), 1);
        let hits = search::query(&db, "example", 10).unwrap();
        assert_eq!(
            hits.iter().filter(|h| h.doc_type == "item").count(),
            1,
            "indexed once"
        );
    }

    #[test]
    fn items_create_does_not_move_from_other_bucket() {
        let (_d, db) = test_db();
        let a = buckets::create(&db, "A", None).unwrap();
        let b = buckets::create(&db, "B", None).unwrap();
        mk(&db, "https://example.com/p", Some(a.id.clone()));
        let again = mk(&db, "https://example.com/p", Some(b.id.clone()));
        assert_eq!(again.bucket_id.as_deref(), Some(a.id.as_str()));
    }

    #[test]
    fn items_create_rejects_invalid_url() {
        let (_d, db) = test_db();
        assert!(matches!(
            create(&db, new("chrome://x", None)),
            Err(StowError::InvalidUrl { .. })
        ));
    }

    #[test]
    fn items_list_inbox_vs_bucket() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "B", None).unwrap();
        let inbox_item = mk(&db, "https://example.com/1", None);
        let bucket_item = mk(&db, "https://example.com/2", Some(b.id.clone()));
        assert_eq!(
            list(&db, ItemFilter::Inbox, true)
                .unwrap()
                .iter()
                .map(|i| &i.id)
                .collect::<Vec<_>>(),
            vec![&inbox_item.id]
        );
        assert_eq!(
            list(&db, ItemFilter::Bucket(b.id.clone()), true).unwrap()[0].id,
            bucket_item.id
        );
        assert_eq!(list(&db, ItemFilter::All, true).unwrap().len(), 2);
        move_to(&db, &inbox_item.id, Some(&b.id)).unwrap();
        assert!(list(&db, ItemFilter::Inbox, true).unwrap().is_empty());
        move_to(&db, &inbox_item.id, None).unwrap();
        assert_eq!(list(&db, ItemFilter::Inbox, true).unwrap().len(), 1);
    }

    #[test]
    fn set_done_hides_from_list_unless_included() {
        let (_d, db) = test_db();
        let a = mk(&db, "https://example.com/a", None);
        let b = mk(&db, "https://example.com/b", None);
        let done = set_done(&db, &a.id, true).unwrap();
        let first_done_at = done.done_at.clone().expect("done_at set");
        assert_eq!(
            set_done(&db, &a.id, true).unwrap().done_at.as_deref(),
            Some(first_done_at.as_str()),
            "marking done twice keeps the first time"
        );
        let open: Vec<_> = list(&db, ItemFilter::Inbox, false)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(open, vec![b.id.clone()]);
        assert_eq!(list(&db, ItemFilter::Inbox, true).unwrap().len(), 2);
        assert!(set_done(&db, &a.id, false).unwrap().done_at.is_none());
        assert_eq!(list(&db, ItemFilter::Inbox, false).unwrap().len(), 2);
        assert!(matches!(
            set_done(&db, "NOPE", true),
            Err(StowError::NotFound { .. })
        ));
    }

    #[test]
    fn count_matches_list() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "B", None).unwrap();
        let a = mk(&db, "https://example.com/1", None);
        mk(&db, "https://example.com/2", Some(b.id.clone()));
        mk(&db, "https://example.com/3", Some(b.id.clone()));
        set_done(&db, &a.id, true).unwrap();
        assert_eq!(count(&db, &ItemFilter::All, true).unwrap(), 3);
        assert_eq!(count(&db, &ItemFilter::All, false).unwrap(), 2);
        assert_eq!(count(&db, &ItemFilter::Inbox, true).unwrap(), 1);
        assert_eq!(count(&db, &ItemFilter::Inbox, false).unwrap(), 0);
        assert_eq!(
            count(&db, &ItemFilter::Bucket(b.id.clone()), false).unwrap(),
            2
        );
    }

    #[test]
    fn find_by_prefix_matches_case_insensitively_and_caps_at_three() {
        let (_d, db) = test_db();
        let items: Vec<Item> = (0..5)
            .map(|i| mk(&db, &format!("https://example.com/{i}"), None))
            .collect();
        let one = &items[0];
        let got = find_by_prefix(&db, &one.id.to_ascii_lowercase()).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, one.id);
        assert_eq!(
            find_by_prefix(&db, &one.id[..2]).unwrap().len(),
            3,
            "LIMIT 3"
        );
        assert!(find_by_prefix(&db, "ZZZZ").unwrap().is_empty());
        assert!(
            find_by_prefix(&db, "%").unwrap().is_empty(),
            "LIKE wildcards are escaped"
        );
        assert!(find_by_prefix(&db, "  ").is_err());
    }

    #[test]
    fn items_title_defaults_to_host() {
        let (_d, db) = test_db();
        let it = mk(&db, "https://docs.example.com/guide", None);
        assert_eq!(it.title.as_deref(), Some("docs.example.com"));
        let given = create(
            &db,
            NewItem {
                url: "https://example.com/t".into(),
                bucket_id: None,
                title: Some("My title".into()),
            },
        )
        .unwrap()
        .value;
        assert_eq!(given.title.as_deref(), Some("My title"));
    }

    #[test]
    fn items_set_metadata_fills_without_clobbering_user_title() {
        let (_d, db) = test_db();
        let it = create(
            &db,
            NewItem {
                url: "https://example.com/t".into(),
                bucket_id: None,
                title: Some("Mine".into()),
            },
        )
        .unwrap()
        .value;
        let patch = MetaPatch {
            title: None,
            description: Some("desc".into()),
            site_name: Some("Example".into()),
            favicon_path: None,
            image_path: None,
        };
        let updated = set_metadata(&db, &it.id, &patch).unwrap();
        assert_eq!(updated.title.as_deref(), Some("Mine"));
        assert_eq!(updated.description.as_deref(), Some("desc"));
        let host_titled = mk(&db, "https://example.com/u", None);
        let patch = MetaPatch {
            title: Some("Fetched".into()),
            ..Default::default()
        };
        assert_eq!(
            set_metadata(&db, &host_titled.id, &patch)
                .unwrap()
                .title
                .as_deref(),
            Some("Fetched")
        );
    }

    #[test]
    fn item_delete_cascades_children_files_and_search() {
        use crate::models::{AiAction, ParentType, Provider, TargetType};
        use crate::repo::{ai_runs, attachments, recordings, reminders, tasks};
        let (dir, db) = test_db();
        let it = mk(&db, "https://example.com/cascade", None);

        let src = dir.path().join("notes.txt");
        std::fs::write(&src, b"attached").unwrap();
        let att = attachments::add(&db, ParentType::Item, &it.id, &src).unwrap();
        let att_dir = db.data_dir().join("attachments").join(&att.id);

        let audio = db.data_dir().join("audio").join("c.wav");
        std::fs::write(&audio, b"RIFF").unwrap();
        let rec =
            recordings::create(&db, ParentType::Item, &it.id, audio.to_str().unwrap(), 10).unwrap();
        recordings::set_transcript(&db, &rec.id, "the quokka speaks").unwrap();

        let run = ai_runs::create(
            &db,
            ParentType::Item,
            &it.id,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "m",
            "h",
        )
        .unwrap();
        ai_runs::complete(&db, &run.id, "axolotl summary").unwrap();

        let task = tasks::create(&db, ParentType::Item, &it.id, "do it").unwrap();
        reminders::create(
            &db,
            TargetType::Task,
            &task.id,
            "2030-01-01T00:00:00Z",
            None,
        )
        .unwrap();
        reminders::create(&db, TargetType::Item, &it.id, "2030-01-01T00:00:00Z", None).unwrap();

        let assets = db.data_dir().join("assets");
        let fav = assets.join(format!("{}-favicon.png", it.id));
        let img = assets.join(format!("{}-image.jpg", it.id));
        let other = assets.join("OTHER-favicon.png");
        for p in [&fav, &img, &other] {
            std::fs::write(p, b"x").unwrap();
        }

        delete(&db, &it.id).unwrap();

        assert!(matches!(get(&db, &it.id), Err(StowError::NotFound { .. })));
        assert!(attachments::list(&db, ParentType::Item, &it.id)
            .unwrap()
            .is_empty());
        assert!(!att_dir.exists(), "attachment dir removed");
        assert!(recordings::list(&db, ParentType::Item, &it.id)
            .unwrap()
            .is_empty());
        assert!(!audio.exists(), "audio file removed");
        assert!(
            search::query(&db, "quokka", 10).unwrap().is_empty(),
            "recording search doc removed"
        );
        assert!(ai_runs::list(&db, ParentType::Item, &it.id)
            .unwrap()
            .is_empty());
        assert!(
            search::query(&db, "axolotl", 10).unwrap().is_empty(),
            "ai_run search doc removed"
        );
        assert!(tasks::list(&db, ParentType::Item, &it.id)
            .unwrap()
            .is_empty());
        assert!(
            reminders::list_for(&db, TargetType::Task, &task.id)
                .unwrap()
                .is_empty(),
            "task reminder removed"
        );
        assert!(reminders::list_for(&db, TargetType::Item, &it.id)
            .unwrap()
            .is_empty());
        assert!(!fav.exists() && !img.exists(), "item assets removed");
        assert!(other.exists(), "other items' assets untouched");
    }

    #[test]
    fn item_delete_is_atomic_on_failure() {
        use crate::models::{AiAction, ParentType, Provider};
        use crate::repo::{ai_runs, attachments, tasks};
        let (dir, db) = test_db();
        let it = mk(&db, "https://example.com/atomic", None);
        let src = dir.path().join("keep.txt");
        std::fs::write(&src, b"keep").unwrap();
        let att = attachments::add(&db, ParentType::Item, &it.id, &src).unwrap();
        tasks::create(&db, ParentType::Item, &it.id, "still here").unwrap();
        ai_runs::create(
            &db,
            ParentType::Item,
            &it.id,
            AiAction::SummarizePage,
            Provider::OpenAi,
            "m",
            "h",
        )
        .unwrap();
        // Fail the cascade halfway: deleting the AI run (after tasks and attachments) aborts.
        db.with(|c| {
            c.execute_batch(
                "CREATE TEMP TRIGGER fail_ai_delete BEFORE DELETE ON main.ai_runs \
                 BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )?;
            Ok(())
        })
        .unwrap();

        let err = delete(&db, &it.id).unwrap_err();
        assert!(err.to_string().contains("injected failure"), "{err}");
        assert!(get(&db, &it.id).is_ok(), "item row kept");
        assert_eq!(
            tasks::list(&db, ParentType::Item, &it.id).unwrap().len(),
            1,
            "tasks kept"
        );
        assert_eq!(
            attachments::list(&db, ParentType::Item, &it.id)
                .unwrap()
                .len(),
            1
        );
        assert!(
            std::path::Path::new(&att.path).is_file(),
            "files untouched when the tx fails"
        );
        assert_eq!(
            search::query(&db, "atomic", 10).unwrap().len(),
            1,
            "search doc kept"
        );
    }

    #[test]
    fn items_snapshot_status_and_notes() {
        let (_d, db) = test_db();
        let it = mk(&db, "https://example.com/n", None);
        set_snapshot_status(&db, &it.id, JobStatus::Failed, Some("boom")).unwrap();
        let got = get(&db, &it.id).unwrap();
        assert_eq!(got.snapshot_status, JobStatus::Failed);
        assert_eq!(got.snapshot_error.as_deref(), Some("boom"));
        set_snapshot_done(&db, &it.id).unwrap();
        let got = get(&db, &it.id).unwrap();
        assert_eq!(got.snapshot_status, JobStatus::Done);
        assert!(got.snapshot_fetched_at.is_some() && got.snapshot_error.is_none());
        set_notes(&db, &it.id, "remember the zebra").unwrap();
        let hits = search::query(&db, "zebra", 10).unwrap();
        assert_eq!(hits[0].item_id.as_deref(), Some(it.id.as_str()));
        delete(&db, &it.id).unwrap();
        assert!(search::query(&db, "zebra", 10).unwrap().is_empty());
        assert!(matches!(
            set_snapshot_done(&db, &it.id),
            Err(StowError::NotFound { .. })
        ));
    }
}
