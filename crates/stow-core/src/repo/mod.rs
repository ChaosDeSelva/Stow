//! Plain functions over `Db`. Every mutation bumps `updated_at` and keeps the FTS index in sync.

pub mod ai_runs;
pub mod attachments;
pub mod buckets;
pub mod items;
pub mod recordings;
pub mod reminders;
pub mod search;
pub mod settings;
pub mod tasks;

use rusqlite::Connection;

use crate::error::EntityKind;
use crate::models::{ParentType, TargetType};
use crate::{Result, StowError};

fn row_exists(c: &Connection, table: &str, id: &str) -> Result<bool> {
    Ok(
        c.prepare_cached(&format!("SELECT count(*) > 0 FROM {table} WHERE id = ?1"))?
            .query_row([id], |r| r.get(0))?,
    )
}

/// `NotFound { kind: Bucket | Item }` unless the parent row exists. Call inside the same
/// transaction as the child insert, so a concurrent delete cannot slip between.
pub(crate) fn require_parent(
    c: &Connection,
    parent_type: ParentType,
    parent_id: &str,
) -> Result<()> {
    let (table, kind) = match parent_type {
        ParentType::Bucket => ("buckets", EntityKind::Bucket),
        ParentType::Item => ("items", EntityKind::Item),
    };
    if row_exists(c, table, parent_id)? {
        Ok(())
    } else {
        Err(StowError::not_found(kind, parent_id))
    }
}

/// [`require_parent`] for reminder targets (buckets, items, tasks).
pub(crate) fn require_target(
    c: &Connection,
    target_type: TargetType,
    target_id: &str,
) -> Result<()> {
    let (table, kind) = match target_type {
        TargetType::Bucket => ("buckets", EntityKind::Bucket),
        TargetType::Item => ("items", EntityKind::Item),
        TargetType::Task => ("tasks", EntityKind::Task),
    };
    if row_exists(c, table, target_id)? {
        Ok(())
    } else {
        Err(StowError::not_found(kind, target_id))
    }
}

#[cfg(test)]
pub(crate) fn test_db() -> (tempfile::TempDir, crate::Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = crate::Db::open(dir.path()).unwrap();
    (dir, db)
}

/// A bucket to hang child rows off in tests.
#[cfg(test)]
pub(crate) fn test_bucket(db: &crate::Db, name: &str) -> crate::models::Bucket {
    buckets::create(db, name, None).unwrap()
}

/// An Inbox item to hang child rows off in tests.
#[cfg(test)]
pub(crate) fn test_item(db: &crate::Db, url: &str) -> crate::models::Item {
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
