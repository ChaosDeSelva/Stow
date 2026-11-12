//! Health report over one data dir (`stw doctor`): SQLite integrity, FTS index
//! consistency, row counts, and the background work a launch would resume. Read-only;
//! the repair for a drifted index is [`crate::repo::search::rebuild`].

use crate::{Db, Result};

/// Rows per table.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Counts {
    /// Non-archived and archived buckets.
    pub buckets: i64,
    /// Items, done or not.
    pub items: i64,
    /// Tasks on buckets and items.
    pub tasks: i64,
    /// Reminders, fired or not.
    pub reminders: i64,
    /// Attachments.
    pub attachments: i64,
    /// Recordings.
    pub recordings: i64,
    /// AI runs.
    pub ai_runs: i64,
    /// Search docs (the FTS content table).
    pub search_docs: i64,
}

/// Background work by state; what [`crate::jobs::resume_pending`] would act on.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Resume {
    /// Snapshots queued but not started.
    pub snapshot_pending: i64,
    /// Snapshots cut off mid-fetch (failed as `interrupted` on the next launch).
    pub snapshot_running: i64,
    /// Transcriptions queued but not started.
    pub transcript_pending: i64,
    /// Transcriptions cut off mid-run.
    pub transcript_running: i64,
    /// AI runs queued but not started.
    pub ai_pending: i64,
    /// AI runs cut off mid-call.
    pub ai_running: i64,
}

/// What [`report`] found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DoctorReport {
    /// `PRAGMA user_version`.
    pub schema_version: i64,
    /// `PRAGMA integrity_check` output: `["ok"]` when the file is sound, else its findings.
    pub integrity: Vec<String>,
    /// `None` when the FTS index agrees with `search_docs`; otherwise SQLite's complaint.
    pub fts_error: Option<String>,
    /// Rows per table.
    pub counts: Counts,
    /// Unfinished background work.
    pub resume: Resume,
}

impl DoctorReport {
    /// True when the database file and the search index are both sound.
    pub fn is_ok(&self) -> bool {
        self.integrity == ["ok"] && self.fts_error.is_none()
    }
}

/// Run every check. Only a broken connection is an `Err`; findings are in the report.
pub fn report(db: &Db) -> Result<DoctorReport> {
    db.with(|c| {
        let schema_version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let integrity = c
            .prepare("PRAGMA integrity_check")?
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // `rank = 1`: for an external-content table this also compares the index against
        // `search_docs`; without it only the index's internal structure is checked.
        let fts_error = c
            .execute(
                "INSERT INTO search (search, rank) VALUES ('integrity-check', 1)",
                [],
            )
            .err()
            .map(|e| e.to_string());
        let count = |table: &str| -> rusqlite::Result<i64> {
            c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
        };
        let state = |table: &str, col: &str, state: &str| -> rusqlite::Result<i64> {
            c.query_row(
                &format!("SELECT count(*) FROM {table} WHERE {col} = ?1"),
                [state],
                |r| r.get(0),
            )
        };
        Ok(DoctorReport {
            schema_version,
            integrity,
            fts_error,
            counts: Counts {
                buckets: count("buckets")?,
                items: count("items")?,
                tasks: count("tasks")?,
                reminders: count("reminders")?,
                attachments: count("attachments")?,
                recordings: count("recordings")?,
                ai_runs: count("ai_runs")?,
                search_docs: count("search_docs")?,
            },
            resume: Resume {
                snapshot_pending: state("items", "snapshot_status", "pending")?,
                snapshot_running: state("items", "snapshot_status", "running")?,
                transcript_pending: state("recordings", "transcript_status", "pending")?,
                transcript_running: state("recordings", "transcript_status", "running")?,
                ai_pending: state("ai_runs", "status", "pending")?,
                ai_running: state("ai_runs", "status", "running")?,
            },
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ParentType;
    use crate::repo::{items, recordings, search, test_bucket, test_db, test_item};

    #[test]
    fn report_counts_rows_and_pending_work() {
        let (_d, db) = test_db();
        let b = test_bucket(&db, "B");
        let it = test_item(&db, "https://example.com/a");
        items::mark_snapshot_pending(&db, &it.id).unwrap();
        let other = test_item(&db, "https://example.com/b");
        items::set_snapshot_status(&db, &other.id, crate::models::JobStatus::Running, None)
            .unwrap();
        recordings::create(&db, ParentType::Bucket, &b.id, "/x.wav", 1).unwrap();

        let r = report(&db).unwrap();
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(r.schema_version, 2);
        assert_eq!(r.integrity, vec!["ok".to_string()]);
        assert_eq!(r.fts_error, None);
        assert_eq!(r.counts.buckets, 1);
        assert_eq!(r.counts.items, 2);
        assert_eq!(r.counts.recordings, 1);
        assert_eq!(r.counts.search_docs, 3, "bucket + two item docs");
        assert_eq!(r.resume.snapshot_pending, 1);
        assert_eq!(r.resume.snapshot_running, 1);
        assert_eq!(r.resume.transcript_pending, 1);
        assert_eq!(r.resume.ai_pending, 0);
        let json = serde_json::to_value(&r).unwrap();
        assert_eq!(json["counts"]["items"], 2);
        assert!(json["fts_error"].is_null());
    }

    #[test]
    fn report_flags_fts_drift_until_rebuilt() {
        let (_d, db) = test_db();
        test_item(&db, "https://example.com/okapi");
        db.with(|c| Ok(c.execute("INSERT INTO search (search) VALUES ('delete-all')", [])?))
            .unwrap();
        let r = report(&db).unwrap();
        assert!(!r.is_ok());
        assert_eq!(
            r.integrity,
            vec!["ok".to_string()],
            "the file itself is fine"
        );
        assert!(r.fts_error.is_some(), "{r:?}");
        search::rebuild(&db).unwrap();
        assert!(report(&db).unwrap().is_ok());
    }
}
