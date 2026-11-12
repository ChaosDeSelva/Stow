//! AI runs: one row per provider call, output indexed as an `ai_run` search doc.

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::EntityKind;
use crate::ids::{new_id, now};
use crate::models::{AiAction, AiRun, JobStatus, ParentType, Provider};
use crate::repo::{recordings::parent_refs_conn, search};
use crate::{Db, Result, StowError};

const COLS: &str = "id, parent_type, parent_id, action, provider, model, input_hash, output_md, \
    status, error, created_at";

fn row(r: &Row) -> rusqlite::Result<AiRun> {
    Ok(AiRun {
        id: r.get(0)?,
        parent_type: r.get(1)?,
        parent_id: r.get(2)?,
        action: r.get(3)?,
        provider: r.get(4)?,
        model: r.get(5)?,
        input_hash: r.get(6)?,
        output_md: r.get(7)?,
        status: r.get(8)?,
        error: r.get(9)?,
        created_at: r.get(10)?,
    })
}

fn returning(c: &Connection, sql: &str, p: impl rusqlite::Params, id: &str) -> Result<AiRun> {
    c.prepare_cached(&format!("{sql} RETURNING {COLS}"))?
        .query_row(p, row)
        .optional()?
        .ok_or_else(|| StowError::not_found(EntityKind::AiRun, id))
}

/// New `Pending` run. The parent must exist (`NotFound` otherwise).
pub fn create(
    db: &Db,
    parent_type: ParentType,
    parent_id: &str,
    action: AiAction,
    provider: Provider,
    model: &str,
    input_hash: &str,
) -> Result<AiRun> {
    let id = new_id();
    db.tx(|c| {
        crate::repo::require_parent(c, parent_type, parent_id)?;
        returning(
            c,
            "INSERT INTO ai_runs (id, parent_type, parent_id, action, provider, model, input_hash, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, parent_type, parent_id, action, provider, model, input_hash, JobStatus::Pending, now()],
            &id,
        )
    })
}

/// One run by id.
pub fn get(db: &Db, id: &str) -> Result<AiRun> {
    db.with(|c| {
        c.prepare_cached(&format!("SELECT {COLS} FROM ai_runs WHERE id = ?1"))?
            .query_row([id], row)
            .optional()?
            .ok_or_else(|| StowError::not_found(EntityKind::AiRun, id))
    })
}

/// Newest first.
pub fn list(db: &Db, parent_type: ParentType, parent_id: &str) -> Result<Vec<AiRun>> {
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM ai_runs WHERE parent_type = ?1 AND parent_id = ?2 ORDER BY created_at DESC, id DESC"
        ))?;
        let v = st
            .query_map(params![parent_type, parent_id], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

fn set_status(db: &Db, id: &str, status: JobStatus, error: Option<&str>) -> Result<AiRun> {
    db.with(|c| {
        returning(
            c,
            "UPDATE ai_runs SET status = ?2, error = ?3 WHERE id = ?1",
            params![id, status, error],
            id,
        )
    })
}

/// Mark `Running`.
pub fn start(db: &Db, id: &str) -> Result<AiRun> {
    set_status(db, id, JobStatus::Running, None)
}

/// Store the output, mark Done, and index it under the action name, atomically.
pub fn complete(db: &Db, id: &str, output_md: &str) -> Result<AiRun> {
    db.tx(|t| {
        let run = returning(
            t,
            "UPDATE ai_runs SET output_md = ?2, status = ?3, error = NULL WHERE id = ?1",
            params![id, output_md, JobStatus::Done],
            id,
        )?;
        let (item_id, bucket_id) = parent_refs_conn(t, run.parent_type, &run.parent_id)?;
        search::index_conn(
            t,
            "ai_run",
            id,
            item_id.as_deref(),
            bucket_id.as_deref(),
            run.action.as_str(),
            output_md,
        )?;
        Ok(run)
    })
}

/// Mark `Failed` with `err`.
pub fn fail(db: &Db, id: &str, err: &str) -> Result<AiRun> {
    set_status(db, id, JobStatus::Failed, Some(err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AiAction, JobStatus, ParentType, Provider};
    use crate::repo::{search, test_bucket, test_db, test_item};

    #[test]
    #[allow(clippy::many_single_char_names)] // one letter per lifecycle stage reads fine here
    fn ai_run_lifecycle() {
        let (_d, db) = test_db();
        let i1 = test_item(&db, "https://example.com/1").id;
        let b1 = test_bucket(&db, "B").id;
        let r = create(
            &db,
            ParentType::Item,
            &i1,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "claude-sonnet-5-5",
            "abc123",
        )
        .unwrap();
        assert_eq!(r.status, JobStatus::Pending);
        assert_eq!(r.action, AiAction::SummarizePage);
        assert_eq!(r.provider, Provider::Anthropic);
        assert_eq!(r.model, "claude-sonnet-5-5");
        assert_eq!(r.input_hash, "abc123");
        assert!(r.output_md.is_none());
        assert!(r.error.is_none());

        let s = start(&db, &r.id).unwrap();
        assert_eq!(s.status, JobStatus::Running);

        let d = complete(&db, &r.id, "The page is about **axolotls**.").unwrap();
        assert_eq!(d.status, JobStatus::Done);
        assert_eq!(
            d.output_md.as_deref(),
            Some("The page is about **axolotls**.")
        );
        assert!(d.error.is_none());

        let hits = search::query(&db, "axolotl", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_type, "ai_run");
        assert_eq!(hits[0].doc_id, r.id);
        assert_eq!(hits[0].title, "summarize_page");
        assert_eq!(hits[0].item_id.as_deref(), Some(i1.as_str()));
        assert_eq!(hits[0].bucket_id, None);

        let r2 = create(
            &db,
            ParentType::Bucket,
            &b1,
            AiAction::SuggestTasks,
            Provider::OpenAi,
            "gpt-5-mini",
            "h2",
        )
        .unwrap();
        let f = fail(&db, &r2.id, "rate limited").unwrap();
        assert_eq!(f.status, JobStatus::Failed);
        assert_eq!(f.error.as_deref(), Some("rate limited"));
        assert!(
            search::query(&db, "rate", 10).unwrap().is_empty(),
            "failures are not indexed"
        );

        let r3 = create(
            &db,
            ParentType::Bucket,
            &b1,
            AiAction::SummarizeNotes,
            Provider::OpenAi,
            "gpt-5-mini",
            "h3",
        )
        .unwrap();
        complete(&db, &r3.id, "pangolin notes").unwrap();
        let hits = search::query(&db, "pangolin", 10).unwrap();
        assert_eq!(hits[0].bucket_id.as_deref(), Some(b1.as_str()));
        assert_eq!(hits[0].item_id, None);

        let l = list(&db, ParentType::Bucket, &b1).unwrap();
        assert_eq!(
            l.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec![r3.id.as_str(), r2.id.as_str()],
            "newest first"
        );
        assert_eq!(list(&db, ParentType::Item, &i1).unwrap().len(), 1);
        assert_eq!(get(&db, &r.id).unwrap(), d);
        assert!(matches!(
            get(&db, "nope"),
            Err(StowError::NotFound {
                kind: EntityKind::AiRun,
                ..
            })
        ));
        assert!(matches!(
            start(&db, "nope"),
            Err(StowError::NotFound {
                kind: EntityKind::AiRun,
                ..
            })
        ));
    }

    #[test]
    fn ai_run_create_rejects_missing_parent() {
        let (_d, db) = test_db();
        for (pt, kind) in [
            (ParentType::Item, EntityKind::Item),
            (ParentType::Bucket, EntityKind::Bucket),
        ] {
            let err = create(
                &db,
                pt,
                "01NOPEXXXXXXXXXXXXXXXXXXXX",
                AiAction::SummarizePage,
                Provider::Anthropic,
                "m",
                "h",
            )
            .unwrap_err();
            assert!(
                matches!(&err, StowError::NotFound { kind: k, .. } if *k == kind),
                "{err:?}"
            );
        }
        let n: i64 = db
            .with(|c| Ok(c.query_row("SELECT count(*) FROM ai_runs", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0);
    }
}
