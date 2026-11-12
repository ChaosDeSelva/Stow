//! Full-text search. Docs live in the `search_docs` table; the external-content FTS5
//! table `search` indexes their title/body and is kept in sync by triggers.
//! `doc_type` ∈ bucket | item | snapshot | recording | `ai_run`.

use rusqlite::{params, Connection};

use crate::models::SearchHit;
use crate::{Db, Result};

/// Insert or replace the doc `(doc_type, doc_id)`.
pub(crate) fn index_conn(
    c: &Connection,
    doc_type: &str,
    doc_id: &str,
    item_id: Option<&str>,
    bucket_id: Option<&str>,
    title: &str,
    body: &str,
) -> Result<()> {
    c.prepare_cached(
        "INSERT INTO search_docs (doc_type, doc_id, item_id, bucket_id, title, body) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(doc_type, doc_id) DO UPDATE SET item_id = excluded.item_id, \
           bucket_id = excluded.bucket_id, title = excluded.title, body = excluded.body",
    )?
    .execute(params![doc_type, doc_id, item_id, bucket_id, title, body])?;
    Ok(())
}

/// Index (or re-index) one doc.
pub fn index(
    db: &Db,
    doc_type: &str,
    doc_id: &str,
    item_id: Option<&str>,
    bucket_id: Option<&str>,
    title: &str,
    body: &str,
) -> Result<()> {
    db.with(|c| index_conn(c, doc_type, doc_id, item_id, bucket_id, title, body))
}

pub(crate) fn set_item_bucket_conn(
    c: &Connection,
    item_id: &str,
    bucket_id: Option<&str>,
) -> Result<()> {
    c.prepare_cached("UPDATE search_docs SET bucket_id = ?2 WHERE item_id = ?1")?
        .execute(params![item_id, bucket_id])?;
    Ok(())
}

/// Re-point every search doc of an item at a new bucket (snapshots, recordings, AI runs too).
pub fn set_item_bucket(db: &Db, item_id: &str, bucket_id: Option<&str>) -> Result<()> {
    db.with(|c| set_item_bucket_conn(c, item_id, bucket_id))
}

pub(crate) fn remove_conn(c: &Connection, doc_type: &str, doc_id: &str) -> Result<()> {
    c.prepare_cached("DELETE FROM search_docs WHERE doc_type = ?1 AND doc_id = ?2")?
        .execute(params![doc_type, doc_id])?;
    Ok(())
}

/// Remove one doc; a missing doc is fine.
pub fn remove(db: &Db, doc_type: &str, doc_id: &str) -> Result<()> {
    db.with(|c| remove_conn(c, doc_type, doc_id))
}

/// Rebuild the FTS index from `search_docs` (repairs a damaged or out-of-sync index).
pub fn rebuild(db: &Db) -> Result<()> {
    db.with(|c| {
        c.execute("INSERT INTO search (search) VALUES ('rebuild')", [])?;
        Ok(())
    })
}

/// Turn free text into a safe FTS5 query: each word becomes a quoted prefix token.
fn fts_query(q: &str) -> String {
    q.split_whitespace()
        .map(|w| format!("\"{}\"*", w.replace('"', "")))
        .filter(|w| w != "\"\"*")
        .collect::<Vec<_>>()
        .join(" ")
}

/// Best matches first (title weighted 2×), at most `limit`.
pub fn query(db: &Db, q: &str, limit: usize) -> Result<Vec<SearchHit>> {
    let fq = fts_query(q);
    if fq.is_empty() {
        return Ok(vec![]);
    }
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    db.with(|c| {
        let mut st = c.prepare_cached(
            "SELECT d.doc_type, d.doc_id, d.item_id, d.bucket_id, d.title, \
                    snippet(search, 1, '[', ']', '…', 12) \
             FROM search JOIN search_docs d ON d.id = search.rowid \
             WHERE search MATCH ?1 ORDER BY bm25(search, 2.0, 1.0) LIMIT ?2",
        )?;
        let v = st
            .query_map(params![fq, limit], |r| {
                Ok(SearchHit {
                    doc_type: r.get(0)?,
                    doc_id: r.get(1)?,
                    item_id: r.get(2)?,
                    bucket_id: r.get(3)?,
                    title: r.get(4)?,
                    snippet: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{buckets, items, test_db};

    #[test]
    fn search_finds_item_by_notes_and_returns_item_id() {
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Marketing plan", None).unwrap();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/launch".into(),
                bucket_id: Some(b.id.clone()),
                title: Some("Launch checklist".into()),
            },
        )
        .unwrap()
        .value;
        items::set_notes(&db, &it.id, "talk to the designers about colours").unwrap();
        let hits = query(&db, "designer", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_type, "item");
        assert_eq!(hits[0].item_id.as_deref(), Some(it.id.as_str()));
        assert_eq!(hits[0].bucket_id.as_deref(), Some(b.id.as_str()));
        assert!(hits[0].snippet.contains("designers"));
        let hits = query(&db, "marketing", 10).unwrap();
        assert_eq!(hits[0].doc_type, "bucket");
        assert_eq!(hits[0].bucket_id.as_deref(), Some(b.id.as_str()));
        assert!(
            query(&db, "\"unbalanced", 10).is_ok(),
            "bad fts syntax must not error"
        );
    }

    #[test]
    fn index_replaces_and_remove_deletes() {
        let (_d, db) = test_db();
        index(&db, "snapshot", "i1", Some("i1"), None, "T", "alpha beta").unwrap();
        index(&db, "snapshot", "i1", Some("i1"), None, "T", "gamma").unwrap();
        assert!(query(&db, "alpha", 10).unwrap().is_empty());
        assert_eq!(query(&db, "gamma", 10).unwrap().len(), 1);
        remove(&db, "snapshot", "i1").unwrap();
        assert!(query(&db, "gamma", 10).unwrap().is_empty());
    }

    #[test]
    fn search_rebuild_restores_index() {
        let (_d, db) = test_db();
        index(
            &db,
            "snapshot",
            "i1",
            Some("i1"),
            None,
            "Okapi",
            "forest giraffe",
        )
        .unwrap();
        db.with(|c| Ok(c.execute("INSERT INTO search (search) VALUES ('delete-all')", [])?))
            .unwrap();
        assert!(query(&db, "okapi", 10).unwrap().is_empty(), "index wiped");
        rebuild(&db).unwrap();
        let hits = query(&db, "okapi", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_id, "i1");
        assert_eq!(query(&db, "giraffe", 10).unwrap().len(), 1);
    }

    #[test]
    fn fts_query_escaping_handles_operators() {
        let (_d, db) = test_db();
        index(
            &db,
            "snapshot",
            "i1",
            Some("i1"),
            None,
            "Boolean logic",
            "cats AND dogs OR birds NOT fish (grouped) star* \"quoted\" near",
        )
        .unwrap();
        for q in [
            "AND",
            "OR",
            "NOT",
            "(",
            ")",
            "*",
            "\"",
            "cats AND",
            "OR dogs",
            "NOT fish",
            "(grouped",
            "grouped)",
            "star*",
            "\"quoted",
            "quoted\"",
            "NEAR(cats dogs)",
            "cats -dogs",
            "title:cats",
            "^cats",
            "a\"b\"c",
            "\"\"\"",
            "* * *",
            "cats + dogs",
        ] {
            let r = query(&db, q, 10);
            assert!(r.is_ok(), "{q:?}: {:?}", r.err());
        }
        // Operators are matched as plain words, not interpreted.
        assert_eq!(
            query(&db, "cats NOT", 10).unwrap().len(),
            1,
            "NOT is a word"
        );
        assert_eq!(query(&db, "fish OR", 10).unwrap().len(), 1);
        assert_eq!(query(&db, "grouped)", 10).unwrap().len(), 1);
        assert!(
            query(&db, "cats NOT zebra", 10).unwrap().is_empty(),
            "all words required"
        );
        assert!(query(&db, "* ( ) \"", 10).unwrap().is_empty());
    }
}
