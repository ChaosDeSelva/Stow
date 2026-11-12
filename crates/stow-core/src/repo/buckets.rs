//! Buckets: intent-named piles of items, with their own notes, tasks, and reminders.

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Deserialize;

use crate::error::EntityKind;
use crate::ids::{new_id, now};
use crate::models::{Bucket, ParentType};
use crate::repo::items::OwnedFiles;
use crate::repo::search;
use crate::{Db, Result, StowError};

/// Partial update for [`update`]. JSON: omit a key to keep it.
#[derive(Debug, Default, Clone, serde::Deserialize)]
pub struct BucketPatch {
    /// New name.
    pub name: Option<String>,
    /// Missing = keep, `null` = clear, string = set.
    #[serde(default, deserialize_with = "double_option")]
    #[allow(clippy::option_option)] // keep / clear / set are three distinct states
    pub emoji: Option<Option<String>>,
    /// New description (Markdown).
    pub description_md: Option<String>,
    /// New notes (Markdown).
    pub notes_md: Option<String>,
}

/// serde treats `null` and a missing field the same for `Option<Option<T>>`; this keeps them apart.
#[allow(clippy::option_option)]
fn double_option<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Option<String>>, D::Error> {
    Ok(Some(Option::<String>::deserialize(d)?))
}

const COLS: &str =
    "id, name, emoji, description_md, notes_md, position, created_at, updated_at, archived_at";

fn row(r: &Row) -> rusqlite::Result<Bucket> {
    Ok(Bucket {
        id: r.get(0)?,
        name: r.get(1)?,
        emoji: r.get(2)?,
        description_md: r.get(3)?,
        notes_md: r.get(4)?,
        position: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        archived_at: r.get(8)?,
    })
}

fn not_found(id: &str) -> StowError {
    StowError::not_found(EntityKind::Bucket, id)
}

fn reindex_conn(c: &Connection, b: &Bucket) -> Result<()> {
    let body = format!("{}\n{}", b.description_md, b.notes_md);
    search::index_conn(c, "bucket", &b.id, None, Some(&b.id), &b.name, &body)
}

fn require_name(name: &str) -> Result<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(StowError::invalid("name", name));
    }
    Ok(name)
}

/// New bucket at the end of the list.
pub fn create(db: &Db, name: &str, emoji: Option<&str>) -> Result<Bucket> {
    let name = require_name(name)?;
    let id = new_id();
    let ts = now();
    db.tx(|t| {
        let b = t
            .prepare_cached(&format!(
                "INSERT INTO buckets (id, name, emoji, position, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), 0) + 1 FROM buckets), ?4, ?4) \
                 RETURNING {COLS}"
            ))?
            .query_row(params![id, name, emoji, ts], row)?;
        reindex_conn(t, &b)?;
        Ok(b)
    })
}

fn get_conn(c: &Connection, id: &str) -> Result<Bucket> {
    c.prepare_cached(&format!("SELECT {COLS} FROM buckets WHERE id = ?1"))?
        .query_row([id], row)
        .optional()?
        .ok_or_else(|| not_found(id))
}

/// One bucket by id (archived ones included).
pub fn get(db: &Db, id: &str) -> Result<Bucket> {
    db.with(|c| get_conn(c, id))
}

/// Non-archived buckets by position.
pub fn list(db: &Db) -> Result<Vec<Bucket>> {
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM buckets WHERE archived_at IS NULL ORDER BY position, created_at"
        ))?;
        let v = st
            .query_map([], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

/// Case-insensitive lookup among non-archived buckets.
pub fn find_by_name(db: &Db, name: &str) -> Result<Option<Bucket>> {
    db.with(|c| {
        Ok(c.prepare_cached(&format!(
            "SELECT {COLS} FROM buckets WHERE archived_at IS NULL AND lower(name) = lower(?1) LIMIT 1"
        ))?
        .query_row([name.trim()], row)
        .optional()?)
    })
}

/// Apply a [`BucketPatch`] and re-index.
pub fn update(db: &Db, id: &str, patch: &BucketPatch) -> Result<Bucket> {
    let name = patch.name.as_deref().map(require_name).transpose()?;
    db.tx(|t| {
        let (set_emoji, emoji) = match &patch.emoji {
            None => (false, None),
            Some(e) => (true, e.as_deref()),
        };
        let b = t
            .prepare_cached(&format!(
                "UPDATE buckets SET name = COALESCE(?2, name), \
                   emoji = CASE WHEN ?3 THEN ?4 ELSE emoji END, \
                   description_md = COALESCE(?5, description_md), notes_md = COALESCE(?6, notes_md), \
                   updated_at = ?7 WHERE id = ?1 RETURNING {COLS}"
            ))?
            .query_row(
                params![id, name, set_emoji, emoji, patch.description_md, patch.notes_md, now()],
                row,
            )
            .optional()?
            .ok_or_else(|| not_found(id))?;
        reindex_conn(t, &b)?;
        Ok(b)
    })
}

/// Hide the bucket from lists and search; its rows are kept.
pub fn archive(db: &Db, id: &str) -> Result<()> {
    db.tx(|t| {
        let n = t
            .prepare_cached("UPDATE buckets SET archived_at = ?2, updated_at = ?2 WHERE id = ?1")?
            .execute(params![id, now()])?;
        if n == 0 {
            return Err(not_found(id));
        }
        search::remove_conn(t, "bucket", id)
    })
}

/// Deletes the bucket and its own children (tasks, reminders, attachments, recordings,
/// AI runs) in one transaction; its items fall back to the Inbox with theirs intact.
/// Attachment dirs and audio files are removed after the commit.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    let files = db.tx(|t| {
        get_conn(t, id)?;
        let files = OwnedFiles::collect(t, db.data_dir(), ParentType::Bucket, id)?;
        t.prepare_cached("DELETE FROM buckets WHERE id = ?1")?
            .execute([id])?;
        Ok(files)
    })?;
    files.remove();
    Ok(())
}

/// Set positions to the order of `ids` (1-based), atomically.
pub fn reorder(db: &Db, ids: &[String]) -> Result<()> {
    let ts = now();
    db.tx(|t| {
        let mut st =
            t.prepare_cached("UPDATE buckets SET position = ?2, updated_at = ?3 WHERE id = ?1")?;
        for (pos, id) in (1_i64..).zip(ids) {
            st.execute(params![id, pos, ts])?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{items, test_db};

    #[test]
    fn buckets_create_list_order() {
        let (_d, db) = test_db();
        let a = create(&db, "Blog", Some("✍️")).unwrap();
        let b = create(&db, "Watch", None).unwrap();
        let list = list(&db).unwrap();
        assert_eq!(
            list.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(),
            vec![a.id.as_str(), b.id.as_str()]
        );
        assert_eq!(list[0].emoji.as_deref(), Some("✍️"));
        assert!(list[1].position > list[0].position);
        reorder(&db, &[b.id.clone(), a.id.clone()]).unwrap();
        let list = super::list(&db).unwrap();
        assert_eq!(list[0].id, b.id);
    }

    #[test]
    fn bucket_update_and_archive() {
        let (_d, db) = test_db();
        let a = create(&db, "Blog", None).unwrap();
        let patch = BucketPatch {
            name: Some("Blog posts".into()),
            notes_md: Some("# notes".into()),
            ..Default::default()
        };
        let updated = update(&db, &a.id, &patch).unwrap();
        assert_eq!(updated.name, "Blog posts");
        assert_eq!(updated.notes_md, "# notes");
        assert!(updated.updated_at >= a.updated_at);
        archive(&db, &a.id).unwrap();
        assert!(list(&db).unwrap().is_empty());
        assert!(get(&db, &a.id).unwrap().archived_at.is_some());
    }

    #[test]
    fn bucket_delete_cascades_own_children() {
        use crate::models::{AiAction, ParentType, Provider, TargetType};
        use crate::repo::{ai_runs, attachments, recordings, reminders, search, tasks};
        let (dir, db) = test_db();
        let b = create(&db, "Blog", None).unwrap();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/keep".into(),
                bucket_id: Some(b.id.clone()),
                title: None,
            },
        )
        .unwrap()
        .value;

        let src = dir.path().join("brief.txt");
        std::fs::write(&src, b"brief").unwrap();
        let att = attachments::add(&db, ParentType::Bucket, &b.id, &src).unwrap();
        let att_dir = db.data_dir().join("attachments").join(&att.id);
        // An attachment on the bucket's item must survive: the item only moves to Inbox.
        let item_att = attachments::add(&db, ParentType::Item, &it.id, &src).unwrap();

        let audio = db.data_dir().join("audio").join("b.wav");
        std::fs::write(&audio, b"RIFF").unwrap();
        let rec = recordings::create(&db, ParentType::Bucket, &b.id, audio.to_str().unwrap(), 10)
            .unwrap();
        recordings::set_transcript(&db, &rec.id, "wombat memo").unwrap();

        let run = ai_runs::create(
            &db,
            ParentType::Bucket,
            &b.id,
            AiAction::SuggestTasks,
            Provider::Anthropic,
            "m",
            "h",
        )
        .unwrap();
        ai_runs::complete(&db, &run.id, "pangolin tasks").unwrap();

        let task = tasks::create(&db, ParentType::Bucket, &b.id, "outline").unwrap();
        reminders::create(
            &db,
            TargetType::Task,
            &task.id,
            "2030-01-01T00:00:00Z",
            None,
        )
        .unwrap();

        delete(&db, &b.id).unwrap();

        assert!(matches!(
            get(&db, &b.id),
            Err(crate::StowError::NotFound { .. })
        ));
        assert_eq!(items::get(&db, &it.id).unwrap().bucket_id, None);
        assert_eq!(
            attachments::list(&db, ParentType::Item, &it.id)
                .unwrap()
                .len(),
            1
        );
        assert!(std::path::Path::new(&item_att.path).is_file());
        assert!(attachments::list(&db, ParentType::Bucket, &b.id)
            .unwrap()
            .is_empty());
        assert!(!att_dir.exists(), "bucket attachment dir removed");
        assert!(recordings::list(&db, ParentType::Bucket, &b.id)
            .unwrap()
            .is_empty());
        assert!(!audio.exists(), "audio file removed");
        assert!(search::query(&db, "wombat", 10).unwrap().is_empty());
        assert!(ai_runs::list(&db, ParentType::Bucket, &b.id)
            .unwrap()
            .is_empty());
        assert!(search::query(&db, "pangolin", 10).unwrap().is_empty());
        assert!(tasks::list(&db, ParentType::Bucket, &b.id)
            .unwrap()
            .is_empty());
        assert!(
            reminders::list_for(&db, TargetType::Task, &task.id)
                .unwrap()
                .is_empty(),
            "task reminder removed"
        );
    }

    #[test]
    fn bucket_patch_emoji_clear_and_keep_in_db() {
        let (_d, db) = test_db();
        let a = create(&db, "Blog", Some("✍️")).unwrap();
        let kept = update(
            &db,
            &a.id,
            &BucketPatch {
                name: Some("B2".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(kept.emoji.as_deref(), Some("✍️"));
        let cleared = update(
            &db,
            &a.id,
            &BucketPatch {
                emoji: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(cleared.emoji, None);
        assert_eq!(cleared.name, "B2");
        assert!(matches!(
            update(&db, "NOPE", &BucketPatch::default()),
            Err(StowError::NotFound { .. })
        ));
        assert!(matches!(
            create(&db, "  ", None),
            Err(StowError::InvalidInput { field: "name", .. })
        ));
    }

    #[test]
    fn bucket_patch_emoji_null_clears_and_missing_keeps() {
        let keep: BucketPatch = serde_json::from_str(r#"{"name":"x"}"#).unwrap();
        assert!(keep.emoji.is_none(), "missing key = leave unchanged");
        let clear: BucketPatch = serde_json::from_str(r#"{"emoji":null}"#).unwrap();
        assert_eq!(clear.emoji, Some(None), "explicit null = clear");
        let set: BucketPatch = serde_json::from_str(r#"{"emoji":"📦"}"#).unwrap();
        assert_eq!(set.emoji, Some(Some("📦".to_string())));
    }

    #[test]
    fn bucket_delete_clears_bucket_id_on_search_docs() {
        let (_d, db) = test_db();
        let a = create(&db, "A", None).unwrap();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/d".into(),
                bucket_id: Some(a.id.clone()),
                title: Some("Doc".into()),
            },
        )
        .unwrap()
        .value;
        crate::repo::search::index(
            &db,
            "snapshot",
            &it.id,
            Some(&it.id),
            Some(&a.id),
            "T",
            "walrus body",
        )
        .unwrap();
        delete(&db, &a.id).unwrap();
        let hits = crate::repo::search::query(&db, "walrus", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].bucket_id, None);
    }

    #[test]
    fn bucket_delete_moves_items_to_inbox() {
        let (_d, db) = test_db();
        let a = create(&db, "Blog", None).unwrap();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/x".into(),
                bucket_id: Some(a.id.clone()),
                title: None,
            },
        )
        .unwrap()
        .value;
        delete(&db, &a.id).unwrap();
        assert!(matches!(
            get(&db, &a.id),
            Err(crate::StowError::NotFound { .. })
        ));
        assert_eq!(items::get(&db, &it.id).unwrap().bucket_id, None);
    }
}
