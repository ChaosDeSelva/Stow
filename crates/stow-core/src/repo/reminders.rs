//! Reminders on buckets, items, and tasks. "Effective time" is `COALESCE(snoozed_until, fire_at)`.
//! Every timestamp is normalized with [`parse_rfc3339`] on the way in, so string order in
//! SQL equals time order.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::EntityKind;
use crate::ids::{new_id, now, parse_rfc3339};
use crate::models::{Reminder, TargetType};
use crate::{Db, Result, StowError};

const COLS: &str =
    "id, target_type, target_id, fire_at, message, fired_at, snoozed_until, created_at";
const EFFECTIVE: &str = "COALESCE(snoozed_until, fire_at)";

fn row(r: &Row) -> rusqlite::Result<Reminder> {
    Ok(Reminder {
        id: r.get(0)?,
        target_type: r.get(1)?,
        target_id: r.get(2)?,
        fire_at: r.get(3)?,
        message: r.get(4)?,
        fired_at: r.get(5)?,
        snoozed_until: r.get(6)?,
        created_at: r.get(7)?,
    })
}

fn not_found(id: &str) -> StowError {
    StowError::not_found(EntityKind::Reminder, id)
}

fn returning(c: &Connection, sql: &str, p: impl rusqlite::Params, id: &str) -> Result<Reminder> {
    c.prepare_cached(&format!("{sql} RETURNING {COLS}"))?
        .query_row(p, row)
        .optional()?
        .ok_or_else(|| not_found(id))
}

fn select_where(
    db: &Db,
    where_clause: &str,
    order: &str,
    p: impl rusqlite::Params,
) -> Result<Vec<Reminder>> {
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM reminders WHERE {where_clause} ORDER BY {order}"
        ))?;
        let v = st
            .query_map(p, row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

/// New reminder. `fire_at` is any RFC 3339 time; it is stored in canonical UTC. The
/// target must exist (`NotFound` otherwise).
pub fn create(
    db: &Db,
    target_type: TargetType,
    target_id: &str,
    fire_at: &str,
    message: Option<&str>,
) -> Result<Reminder> {
    let fire_at = parse_rfc3339(fire_at)?;
    let id = new_id();
    let message = message.map(str::trim).filter(|m| !m.is_empty());
    db.tx(|c| {
        crate::repo::require_target(c, target_type, target_id)?;
        returning(
            c,
            "INSERT INTO reminders (id, target_type, target_id, fire_at, message, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, target_type, target_id, fire_at, message, now()],
            &id,
        )
    })
}

/// One reminder by id.
pub fn get(db: &Db, id: &str) -> Result<Reminder> {
    db.with(|c| {
        c.prepare_cached(&format!("SELECT {COLS} FROM reminders WHERE id = ?1"))?
            .query_row([id], row)
            .optional()?
            .ok_or_else(|| not_found(id))
    })
}

/// A target's reminders by effective time.
pub fn list_for(db: &Db, target_type: TargetType, target_id: &str) -> Result<Vec<Reminder>> {
    select_where(
        db,
        "target_type = ?1 AND target_id = ?2",
        &format!("{EFFECTIVE}, created_at"),
        params![target_type, target_id],
    )
}

/// Not-yet-fired reminders, soonest first.
pub fn list_upcoming(db: &Db, limit: usize) -> Result<Vec<Reminder>> {
    select_where(
        db,
        "fired_at IS NULL",
        &format!("{EFFECTIVE}, created_at LIMIT ?1"),
        [i64::try_from(limit).unwrap_or(i64::MAX)],
    )
}

/// Reminders whose effective time is at or before `now` and that have not fired since.
pub fn due(db: &Db, now: &str) -> Result<Vec<Reminder>> {
    let now = parse_rfc3339(now)?;
    select_where(
        db,
        &format!("fired_at IS NULL AND {EFFECTIVE} <= ?1"),
        &format!("{EFFECTIVE}, created_at"),
        [now],
    )
}

/// Reminders with effective time in `[start, end)`, fired or not (the Today view).
pub fn between(db: &Db, start: &str, end: &str) -> Result<Vec<Reminder>> {
    let (start, end) = (parse_rfc3339(start)?, parse_rfc3339(end)?);
    select_where(
        db,
        &format!("{EFFECTIVE} >= ?1 AND {EFFECTIVE} < ?2"),
        &format!("{EFFECTIVE}, created_at"),
        [start, end],
    )
}

/// Record that the reminder fired at `now`, unless it already fired (the first mark
/// wins). Returns the row either way; see [`try_mark_fired`] to learn who won.
pub fn mark_fired(db: &Db, id: &str, now: &str) -> Result<Reminder> {
    try_mark_fired(db, id, now)?;
    get(db, id)
}

/// `UPDATE … WHERE id = ?1 AND fired_at IS NULL`: `true` when this call marked the
/// reminder, `false` when another scheduler already had. `NotFound` when there is no row.
pub fn try_mark_fired(db: &Db, id: &str, now: &str) -> Result<bool> {
    let now = parse_rfc3339(now)?;
    db.with(|c| {
        let n = c
            .prepare_cached(
                "UPDATE reminders SET fired_at = ?2 WHERE id = ?1 AND fired_at IS NULL",
            )?
            .execute(params![id, now])?;
        if n > 0 {
            return Ok(true);
        }
        let exists: bool = c
            .prepare_cached("SELECT count(*) > 0 FROM reminders WHERE id = ?1")?
            .query_row([id], |r| r.get(0))?;
        if exists {
            Ok(false)
        } else {
            Err(not_found(id))
        }
    })
}

/// Defer to `until` and re-arm so `due` returns it again once `until` passes.
pub fn snooze(db: &Db, id: &str, until: &str) -> Result<Reminder> {
    let until = parse_rfc3339(until)?;
    db.with(|c| {
        returning(
            c,
            "UPDATE reminders SET snoozed_until = ?2, fired_at = NULL WHERE id = ?1",
            params![id, until],
            id,
        )
    })
}

/// Delete one reminder.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    db.with(|c| {
        let n = c
            .prepare_cached("DELETE FROM reminders WHERE id = ?1")?
            .execute([id])?;
        if n == 0 {
            return Err(not_found(id));
        }
        Ok(())
    })
}

/// Delete every reminder on a target.
pub fn delete_for_target(db: &Db, target_type: TargetType, target_id: &str) -> Result<()> {
    db.with(|c| {
        c.prepare_cached("DELETE FROM reminders WHERE target_type = ?1 AND target_id = ?2")?
            .execute(params![target_type, target_id])?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EntityKind;
    use crate::models::{ParentType, TargetType};
    use crate::repo::{tasks, test_bucket, test_db, test_item};

    /// `(item1, item2, task, bucket)` ids to target.
    fn targets(db: &Db) -> (String, String, String, String) {
        let i1 = test_item(db, "https://example.com/1").id;
        let i2 = test_item(db, "https://example.com/2").id;
        let t1 = tasks::create(db, ParentType::Item, &i1, "t").unwrap().id;
        let b1 = test_bucket(db, "B").id;
        (i1, i2, t1, b1)
    }

    #[test]
    fn reminder_due_excludes_fired_and_future() {
        let (_d, db) = test_db();
        let (i1, _i2, t1, _b1) = targets(&db);
        let past = create(
            &db,
            TargetType::Item,
            &i1,
            "2026-01-01T00:00:00Z",
            Some("old"),
        )
        .unwrap();
        let future = create(&db, TargetType::Item, &i1, "2030-01-01T00:00:00Z", None).unwrap();
        let fired = create(&db, TargetType::Task, &t1, "2026-01-02T00:00:00Z", None).unwrap();
        mark_fired(&db, &fired.id, "2026-01-02T00:00:01Z").unwrap();

        let due = due(&db, "2026-06-01T00:00:00Z").unwrap();
        assert_eq!(
            due.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec![past.id.as_str()]
        );
        assert_eq!(due[0].message.as_deref(), Some("old"));

        let up = list_upcoming(&db, 10).unwrap();
        assert_eq!(
            up.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec![past.id.as_str(), future.id.as_str()]
        );
        assert_eq!(list_upcoming(&db, 1).unwrap().len(), 1);

        let today = between(&db, "2026-01-01T00:00:00Z", "2026-01-03T00:00:00Z").unwrap();
        assert_eq!(
            today.len(),
            2,
            "between includes fired rows and is half-open"
        );
        assert_eq!(
            between(&db, "2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z")
                .unwrap()
                .len(),
            1
        );

        assert_eq!(list_for(&db, TargetType::Item, &i1).unwrap().len(), 2);
        assert_eq!(list_for(&db, TargetType::Task, &t1).unwrap().len(), 1);
        assert!(list_for(&db, TargetType::Bucket, &i1).unwrap().is_empty());
    }

    #[test]
    fn reminder_snooze_defers_and_refires() {
        let (_d, db) = test_db();
        let (_i1, _i2, _t1, b1) = targets(&db);
        let r = create(&db, TargetType::Bucket, &b1, "2026-01-01T00:00:00Z", None).unwrap();
        mark_fired(&db, &r.id, "2026-01-01T00:00:05Z").unwrap();
        assert!(due(&db, "2026-01-01T01:00:00Z").unwrap().is_empty());

        let s = snooze(&db, &r.id, "2026-01-01T02:00:00Z").unwrap();
        assert_eq!(
            s.snoozed_until.as_deref(),
            Some("2026-01-01T02:00:00.000000Z")
        );
        assert!(s.fired_at.is_none());
        assert_eq!(
            s.fire_at, "2026-01-01T00:00:00.000000Z",
            "original fire_at is kept"
        );

        assert!(
            due(&db, "2026-01-01T01:59:59Z").unwrap().is_empty(),
            "deferred past original fire_at"
        );
        assert_eq!(
            due(&db, "2026-01-01T02:00:00Z").unwrap().len(),
            1,
            "refires at snoozed_until"
        );

        // between uses the effective time too.
        assert!(between(&db, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z")
            .unwrap()
            .is_empty());
        assert_eq!(
            between(&db, "2026-01-01T01:00:00Z", "2026-01-01T03:00:00Z")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn reminder_mark_fired_then_due_is_empty() {
        let (_d, db) = test_db();
        let (i1, i2, _t1, _b1) = targets(&db);
        let a = create(&db, TargetType::Item, &i1, "2026-01-01T00:00:00Z", None).unwrap();
        let b = create(&db, TargetType::Item, &i2, "2026-01-01T00:00:00Z", None).unwrap();
        assert_eq!(due(&db, "2026-01-01T00:00:00Z").unwrap().len(), 2);
        mark_fired(&db, &a.id, "2026-01-01T00:00:00Z").unwrap();
        mark_fired(&db, &b.id, "2026-01-01T00:00:00Z").unwrap();
        assert!(due(&db, "2026-01-01T00:00:00Z").unwrap().is_empty());
        assert!(due(&db, "2030-01-01T00:00:00Z").unwrap().is_empty());
        assert!(get(&db, &a.id).unwrap().fired_at.is_some());
        assert!(list_upcoming(&db, 10).unwrap().is_empty());

        delete(&db, &a.id).unwrap();
        assert!(matches!(
            get(&db, &a.id),
            Err(StowError::NotFound {
                kind: EntityKind::Reminder,
                ..
            })
        ));
        assert!(matches!(
            delete(&db, &a.id),
            Err(StowError::NotFound {
                kind: EntityKind::Reminder,
                ..
            })
        ));
        delete_for_target(&db, TargetType::Item, &i2).unwrap();
        assert!(list_for(&db, TargetType::Item, &i2).unwrap().is_empty());
    }

    #[test]
    fn reminder_times_are_normalized_to_utc() {
        let (_d, db) = test_db();
        let (i1, i2, _t1, _b1) = targets(&db);
        // 07:00 at UTC-5 is 12:00Z.
        let r = create(
            &db,
            TargetType::Item,
            &i1,
            "2026-03-01T07:00:00-05:00",
            None,
        )
        .unwrap();
        assert_eq!(r.fire_at, "2026-03-01T12:00:00.000000Z");
        // A naive string compare of the raw input against 11:30Z would call it due.
        assert!(due(&db, "2026-03-01T11:30:00Z").unwrap().is_empty());
        assert!(due(&db, "2026-03-01T06:59:00-05:00").unwrap().is_empty());
        assert_eq!(due(&db, "2026-03-01T12:00:00.000Z").unwrap().len(), 1);
        assert_eq!(
            due(&db, "2026-03-01T13:00:00+01:00").unwrap().len(),
            1,
            "same instant"
        );

        // Ordering against now(): a reminder 1 h in the future at a negative offset.
        let soon = (chrono::Utc::now() + chrono::Duration::hours(1))
            .with_timezone(&chrono::FixedOffset::west_opt(5 * 3600).unwrap())
            .to_rfc3339();
        let f = create(&db, TargetType::Item, &i2, &soon, None).unwrap();
        assert!(f.fire_at.ends_with('Z'));
        assert!(f.fire_at > now(), "{} should be after now", f.fire_at);
        assert!(!due(&db, &now()).unwrap().iter().any(|x| x.id == f.id));

        let s = snooze(&db, &r.id, "2026-03-02T09:00:00+09:00").unwrap();
        assert_eq!(
            s.snoozed_until.as_deref(),
            Some("2026-03-02T00:00:00.000000Z")
        );
        assert_eq!(
            between(&db, "2026-03-01T19:00:00-05:00", "2026-03-02T01:00:00Z")
                .unwrap()
                .len(),
            1
        );
        for bad in [
            create(&db, TargetType::Item, &i1, "tomorrow", None).err(),
            snooze(&db, &r.id, "").err(),
        ] {
            assert_eq!(bad.unwrap().code(), "invalid_input");
        }
    }

    #[test]
    fn reminder_create_rejects_missing_target() {
        let (_d, db) = test_db();
        for (tt, kind) in [
            (TargetType::Item, EntityKind::Item),
            (TargetType::Bucket, EntityKind::Bucket),
            (TargetType::Task, EntityKind::Task),
        ] {
            let err = create(
                &db,
                tt,
                "01NOPEXXXXXXXXXXXXXXXXXXXX",
                "2030-01-01T00:00:00Z",
                None,
            )
            .unwrap_err();
            assert!(
                matches!(&err, StowError::NotFound { kind: k, .. } if *k == kind),
                "{err:?}"
            );
        }
        assert!(list_upcoming(&db, 10).unwrap().is_empty());
    }

    #[test]
    fn mark_fired_only_fires_once() {
        let (_d, db) = test_db();
        let (i1, _, _, _) = targets(&db);
        let r = create(&db, TargetType::Item, &i1, "2026-01-01T00:00:00Z", None).unwrap();
        assert!(try_mark_fired(&db, &r.id, "2026-01-01T00:00:01Z").unwrap());
        assert!(
            !try_mark_fired(&db, &r.id, "2026-01-01T00:00:02Z").unwrap(),
            "second marker loses"
        );
        let got = mark_fired(&db, &r.id, "2026-01-01T00:00:03Z").unwrap();
        assert_eq!(
            got.fired_at.as_deref(),
            Some("2026-01-01T00:00:01.000000Z"),
            "the first fired_at is kept"
        );
        assert!(matches!(
            try_mark_fired(&db, "01NOPEXXXXXXXXXXXXXXXXXXXX", "2026-01-01T00:00:00Z"),
            Err(StowError::NotFound { .. })
        ));
    }
}
