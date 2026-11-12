//! Reminder firing. Pure over the database; the app supplies the notifier.

use crate::models::{Reminder, TargetType};
use crate::repo::{buckets, items, reminders, tasks};
use crate::{Db, Result, StowError};

/// Human title for a reminder's target, for the notification headline.
pub fn target_title(db: &Db, target_type: TargetType, target_id: &str) -> String {
    match target_type {
        TargetType::Bucket => {
            buckets::get(db, target_id).map_or_else(|_| "Bucket".into(), |b| b.name)
        }
        TargetType::Item => items::get(db, target_id)
            .ok()
            .and_then(|i| i.title)
            .unwrap_or_else(|| "Saved link".into()),
        TargetType::Task => tasks::get(db, target_id).map_or_else(|_| "Task".into(), |t| t.title),
    }
}

/// Fire every reminder due at `now`. Each one is marked fired BEFORE `notify` runs,
/// so a crash inside the notifier can never double-fire; one that another scheduler
/// marked, or that was deleted, since the `due` read is skipped. Returns the reminders
/// this call fired.
pub fn fire_due(
    db: &Db,
    now: &str,
    notify: &mut dyn FnMut(&Reminder, &str),
) -> Result<Vec<Reminder>> {
    let due = reminders::due(db, now)?;
    let mut fired = Vec::with_capacity(due.len());
    for r in due {
        match reminders::try_mark_fired(db, &r.id, now) {
            Ok(true) => {}
            Ok(false) | Err(StowError::NotFound { .. }) => continue,
            Err(e) => return Err(e),
        }
        let title = target_title(db, r.target_type, &r.target_id);
        notify(&r, &title);
        fired.push(r);
    }
    Ok(fired)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TargetType;
    use crate::repo::test_db;

    #[test]
    fn due_reminder_is_marked_before_notify() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Blog", None).unwrap();
        let r = reminders::create(
            &db,
            TargetType::Bucket,
            &b.id,
            "2026-01-01T00:00:00Z",
            Some("write"),
        )
        .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            fire_due(&db, "2026-01-01T00:00:30Z", &mut |_, _| {
                panic!("notifier crashed")
            })
            .unwrap();
        }));
        assert!(result.is_err(), "notifier panic should propagate");
        assert!(
            reminders::due(&db, "2026-01-01T00:01:00Z")
                .unwrap()
                .is_empty(),
            "already marked fired"
        );
        let _ = r;
    }

    #[test]
    fn fire_due_uses_fallback_titles_when_targets_are_gone() {
        // Reminders can outlive their target (rows written before the cascade triggers
        // existed, or a target id that never resolved). They must still fire exactly
        // once, with the generic headline, and be marked fired.
        let (_d, db) = test_db();
        // `reminders::create` refuses missing targets, so these legacy rows go in raw.
        let legacy = |id: &str, tt: &str, target: &str, at: &str, msg: Option<&str>| {
            db.with(|c| {
                c.execute(
                    "INSERT INTO reminders (id, target_type, target_id, fire_at, message, created_at) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?4)",
                    rusqlite::params![id, tt, target, at, msg],
                )?;
                Ok(())
            })
            .unwrap();
            reminders::get(&db, id).unwrap()
        };
        let item_r = legacy(
            "01LEGACYREMINDER0000000001",
            "item",
            "01GONEITEMXXXXXXXXXXXXXXXX",
            "2026-01-01T00:00:00.000000Z",
            Some("read it"),
        );
        let bucket_r = legacy(
            "01LEGACYREMINDER0000000002",
            "bucket",
            "01GONEBUCKETXXXXXXXXXXXXXX",
            "2026-01-01T00:00:01.000000Z",
            None,
        );
        let task_r = legacy(
            "01LEGACYREMINDER0000000003",
            "task",
            "01GONETASKXXXXXXXXXXXXXXXX",
            "2026-01-01T00:00:02.000000Z",
            None,
        );
        // An item that exists but whose title was cleared also falls back.
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/untitled".into(),
                bucket_id: None,
                title: None,
            },
        )
        .unwrap()
        .value;
        db.with(|c| {
            c.execute("UPDATE items SET title = NULL WHERE id = ?1", [&it.id])?;
            Ok(())
        })
        .unwrap();
        let untitled_r =
            reminders::create(&db, TargetType::Item, &it.id, "2026-01-01T00:00:03Z", None).unwrap();

        assert_eq!(target_title(&db, TargetType::Item, "nope"), "Saved link");
        assert_eq!(target_title(&db, TargetType::Bucket, "nope"), "Bucket");
        assert_eq!(target_title(&db, TargetType::Task, "nope"), "Task");

        let mut seen = vec![];
        let fired = fire_due(&db, "2026-01-02T00:00:00Z", &mut |r, title| {
            seen.push((r.id.clone(), title.to_string()));
        })
        .unwrap();
        assert_eq!(fired.len(), 4);
        assert_eq!(
            seen,
            vec![
                (item_r.id.clone(), "Saved link".to_string()),
                (bucket_r.id.clone(), "Bucket".to_string()),
                (task_r.id.clone(), "Task".to_string()),
                (untitled_r.id.clone(), "Saved link".to_string()),
            ],
            "soonest first, generic headlines"
        );
        for r in [&item_r, &bucket_r, &task_r, &untitled_r] {
            let got = reminders::get(&db, &r.id).unwrap();
            assert_eq!(
                got.fired_at.as_deref(),
                Some("2026-01-02T00:00:00.000000Z"),
                "{} marked fired",
                r.id
            );
        }
        // Exactly once: a later tick fires nothing.
        let again = fire_due(&db, "2026-01-03T00:00:00Z", &mut |_, _| {
            panic!("fired twice")
        })
        .unwrap();
        assert!(again.is_empty());
    }

    #[test]
    fn fire_due_passes_target_title_and_skips_future() {
        let (_d, db) = test_db();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/a".into(),
                bucket_id: None,
                title: Some("Read me".into()),
            },
        )
        .unwrap()
        .value;
        reminders::create(&db, TargetType::Item, &it.id, "2026-01-01T00:00:00Z", None).unwrap();
        reminders::create(&db, TargetType::Item, &it.id, "2027-01-01T00:00:00Z", None).unwrap();
        let mut seen = vec![];
        let fired = fire_due(&db, "2026-06-01T00:00:00Z", &mut |r, title| {
            seen.push((r.fire_at.clone(), title.to_string()));
        })
        .unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(
            seen,
            vec![(
                "2026-01-01T00:00:00.000000Z".to_string(),
                "Read me".to_string()
            )]
        );
    }

    #[test]
    fn fire_due_does_not_double_notify_when_already_fired() {
        // Two schedulers (two app instances, or the tray and the window) can read the same
        // due list. Whoever marks a reminder first notifies; the other must stay quiet.
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Blog", None).unwrap();
        let first = reminders::create(
            &db,
            TargetType::Bucket,
            &b.id,
            "2026-01-01T00:00:00Z",
            Some("one"),
        )
        .unwrap();
        let second = reminders::create(
            &db,
            TargetType::Bucket,
            &b.id,
            "2026-01-01T00:00:01Z",
            Some("two"),
        )
        .unwrap();
        let mut seen = vec![];
        let fired = fire_due(&db, "2026-01-01T00:01:00Z", &mut |r, _| {
            seen.push(r.id.clone());
            if r.id == first.id {
                // The other scheduler fires `second` between our read and our mark.
                reminders::mark_fired(&db, &second.id, "2026-01-01T00:00:59Z").unwrap();
            }
        })
        .unwrap();
        assert_eq!(
            seen,
            vec![first.id.clone()],
            "second was already fired elsewhere"
        );
        assert_eq!(
            fired.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            vec![first.id.as_str()]
        );
        assert_eq!(
            reminders::get(&db, &second.id).unwrap().fired_at.as_deref(),
            Some("2026-01-01T00:00:59.000000Z"),
            "the other scheduler's mark is kept"
        );
    }

    #[test]
    fn fire_due_skips_a_reminder_deleted_since_the_due_read() {
        // Deleting a reminder (or its target, which cascades) between `due` and the mark
        // must not abort the tick: the rest of the batch still fires.
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Blog", None).unwrap();
        let first = reminders::create(
            &db,
            TargetType::Bucket,
            &b.id,
            "2026-01-01T00:00:00Z",
            Some("one"),
        )
        .unwrap();
        let doomed = reminders::create(
            &db,
            TargetType::Bucket,
            &b.id,
            "2026-01-01T00:00:01Z",
            Some("two"),
        )
        .unwrap();
        let third = reminders::create(
            &db,
            TargetType::Bucket,
            &b.id,
            "2026-01-01T00:00:02Z",
            Some("three"),
        )
        .unwrap();
        let mut seen = vec![];
        let fired = fire_due(&db, "2026-01-01T00:01:00Z", &mut |r, _| {
            seen.push(r.id.clone());
            if r.id == first.id {
                reminders::delete(&db, &doomed.id).unwrap();
            }
        })
        .unwrap();
        assert_eq!(seen, vec![first.id.clone(), third.id.clone()]);
        assert_eq!(fired.len(), 2);
        assert!(matches!(
            reminders::get(&db, &doomed.id),
            Err(StowError::NotFound { .. })
        ));
    }
}
