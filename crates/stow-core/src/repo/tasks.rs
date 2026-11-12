//! Checklist tasks on a bucket or item.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::EntityKind;
use crate::ids::{new_id, now};
use crate::models::{ParentType, Task};
use crate::{Db, Result, StowError};

const COLS: &str = "id, parent_type, parent_id, title, done_at, position, created_at";

fn row(r: &Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        parent_type: r.get(1)?,
        parent_id: r.get(2)?,
        title: r.get(3)?,
        done_at: r.get(4)?,
        position: r.get(5)?,
        created_at: r.get(6)?,
    })
}

fn returning(c: &Connection, sql: &str, p: impl rusqlite::Params, id: &str) -> Result<Task> {
    c.prepare_cached(&format!("{sql} RETURNING {COLS}"))?
        .query_row(p, row)
        .optional()?
        .ok_or_else(|| StowError::not_found(EntityKind::Task, id))
}

fn require_title(title: &str) -> Result<&str> {
    let t = title.trim();
    if t.is_empty() {
        return Err(StowError::invalid("title", title));
    }
    Ok(t)
}

/// New task at the end of the parent's list. The parent must exist (`NotFound` otherwise).
pub fn create(db: &Db, parent_type: ParentType, parent_id: &str, title: &str) -> Result<Task> {
    let title = require_title(title)?;
    let id = new_id();
    db.tx(|c| {
        crate::repo::require_parent(c, parent_type, parent_id)?;
        returning(
            c,
            "INSERT INTO tasks (id, parent_type, parent_id, title, position, created_at) \
             VALUES (?1, ?2, ?3, ?4, \
               (SELECT COALESCE(MAX(position), 0) + 1 FROM tasks WHERE parent_type = ?2 AND parent_id = ?3), ?5)",
            params![id, parent_type, parent_id, title, now()],
            &id,
        )
    })
}

/// One task by id.
pub fn get(db: &Db, id: &str) -> Result<Task> {
    db.with(|c| {
        c.prepare_cached(&format!("SELECT {COLS} FROM tasks WHERE id = ?1"))?
            .query_row([id], row)
            .optional()?
            .ok_or_else(|| StowError::not_found(EntityKind::Task, id))
    })
}

/// The parent's tasks by position.
pub fn list(db: &Db, parent_type: ParentType, parent_id: &str) -> Result<Vec<Task>> {
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM tasks WHERE parent_type = ?1 AND parent_id = ?2 ORDER BY position, created_at"
        ))?;
        let v = st
            .query_map(params![parent_type, parent_id], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

/// Flip done/not done.
pub fn toggle(db: &Db, id: &str) -> Result<Task> {
    db.with(|c| {
        returning(
            c,
            "UPDATE tasks SET done_at = CASE WHEN done_at IS NULL THEN ?2 ELSE NULL END WHERE id = ?1",
            params![id, now()],
            id,
        )
    })
}

/// Change the title.
pub fn rename(db: &Db, id: &str, title: &str) -> Result<Task> {
    let title = require_title(title)?;
    db.with(|c| {
        returning(
            c,
            "UPDATE tasks SET title = ?2 WHERE id = ?1",
            params![id, title],
            id,
        )
    })
}

/// Delete the task; its reminders go with it (cascade trigger).
pub fn delete(db: &Db, id: &str) -> Result<()> {
    db.with(|c| {
        let n = c
            .prepare_cached("DELETE FROM tasks WHERE id = ?1")?
            .execute([id])?;
        if n == 0 {
            return Err(StowError::not_found(EntityKind::Task, id));
        }
        Ok(())
    })
}

/// Set positions to the order of `ids` (1-based), atomically.
pub fn reorder(db: &Db, ids: &[String]) -> Result<()> {
    db.tx(|t| {
        let mut st = t.prepare_cached("UPDATE tasks SET position = ?2 WHERE id = ?1")?;
        for (pos, id) in (1_i64..).zip(ids) {
            st.execute(params![id, pos])?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ParentType;
    use crate::repo::{test_bucket, test_db, test_item};

    #[test]
    fn tasks_toggle_sets_and_clears_done_at() {
        let (_d, db) = test_db();
        let b = test_bucket(&db, "B");
        let t = create(&db, ParentType::Bucket, &b.id, "Write intro").unwrap();
        assert!(t.done_at.is_none());
        let t = toggle(&db, &t.id).unwrap();
        assert!(t.done_at.is_some());
        let t = toggle(&db, &t.id).unwrap();
        assert!(t.done_at.is_none());
    }

    #[test]
    fn tasks_list_rename_reorder_delete() {
        let (_d, db) = test_db();
        let i1 = test_item(&db, "https://example.com/1").id;
        let i2 = test_item(&db, "https://example.com/2").id;
        let a = create(&db, ParentType::Item, &i1, "a").unwrap();
        let b = create(&db, ParentType::Item, &i1, "b").unwrap();
        create(&db, ParentType::Item, &i2, "other").unwrap();
        let l = list(&db, ParentType::Item, &i1).unwrap();
        assert_eq!(
            l.iter().map(|t| t.title.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        reorder(&db, &[b.id.clone(), a.id.clone()]).unwrap();
        assert_eq!(list(&db, ParentType::Item, &i1).unwrap()[0].id, b.id);
        rename(&db, &a.id, "A!").unwrap();
        assert_eq!(get(&db, &a.id).unwrap().title, "A!");
        delete(&db, &a.id).unwrap();
        assert_eq!(list(&db, ParentType::Item, &i1).unwrap().len(), 1);
        assert!(matches!(
            delete(&db, &a.id),
            Err(StowError::NotFound { .. })
        ));
        assert!(matches!(
            rename(&db, &b.id, " "),
            Err(StowError::InvalidInput { .. })
        ));
    }

    #[test]
    fn task_delete_cascades_reminders() {
        use crate::models::TargetType;
        use crate::repo::reminders;
        let (_d, db) = test_db();
        let b = test_bucket(&db, "B");
        let t = create(&db, ParentType::Bucket, &b.id, "x").unwrap();
        reminders::create(&db, TargetType::Task, &t.id, "2030-01-01T00:00:00Z", None).unwrap();
        delete(&db, &t.id).unwrap();
        assert!(reminders::list_for(&db, TargetType::Task, &t.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn task_create_rejects_missing_parent() {
        use crate::error::EntityKind;
        let (_d, db) = test_db();
        let err = create(&db, ParentType::Item, "01NOPEITEMXXXXXXXXXXXXXXXX", "x").unwrap_err();
        assert!(
            matches!(
                err,
                StowError::NotFound {
                    kind: EntityKind::Item,
                    ..
                }
            ),
            "{err:?}"
        );
        let err = create(&db, ParentType::Bucket, "01NOPEBUCKETXXXXXXXXXXXXXX", "x").unwrap_err();
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
        let n: i64 = db
            .with(|c| Ok(c.query_row("SELECT count(*) FROM tasks", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0, "no orphan task rows");
    }
}
