//! Voice recordings with Whisper transcripts. The transcript is indexed as a `recording` search doc.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{EntityKind, IoContext};
use crate::ids::{new_id, now};
use crate::models::{JobStatus, ParentType, Recording};
use crate::repo::search;
use crate::{Db, Result, StowError};

const COLS: &str = "id, parent_type, parent_id, audio_path, duration_ms, transcript_md, \
    transcript_status, transcript_error, created_at";

fn row(r: &Row) -> rusqlite::Result<Recording> {
    Ok(Recording {
        id: r.get(0)?,
        parent_type: r.get(1)?,
        parent_id: r.get(2)?,
        audio_path: r.get(3)?,
        duration_ms: r.get(4)?,
        transcript_md: r.get(5)?,
        transcript_status: r.get(6)?,
        transcript_error: r.get(7)?,
        created_at: r.get(8)?,
    })
}

fn not_found(id: &str) -> StowError {
    StowError::not_found(EntityKind::Recording, id)
}

/// Record a finished WAV; transcription starts as `Pending`. The parent must exist
/// (`NotFound` otherwise).
pub fn create(
    db: &Db,
    parent_type: ParentType,
    parent_id: &str,
    audio_path: &str,
    duration_ms: i64,
) -> Result<Recording> {
    let id = new_id();
    db.tx(|c| {
        crate::repo::require_parent(c, parent_type, parent_id)?;
        Ok(c.prepare_cached(&format!(
            "INSERT INTO recordings (id, parent_type, parent_id, audio_path, duration_ms, transcript_status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING {COLS}"
        ))?
        .query_row(
            params![id, parent_type, parent_id, audio_path, duration_ms, JobStatus::Pending, now()],
            row,
        )?)
    })
}

/// One recording by id.
pub fn get(db: &Db, id: &str) -> Result<Recording> {
    db.with(|c| {
        c.prepare_cached(&format!("SELECT {COLS} FROM recordings WHERE id = ?1"))?
            .query_row([id], row)
            .optional()?
            .ok_or_else(|| not_found(id))
    })
}

/// The parent's recordings, newest first.
pub fn list(db: &Db, parent_type: ParentType, parent_id: &str) -> Result<Vec<Recording>> {
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM recordings WHERE parent_type = ?1 AND parent_id = ?2 ORDER BY created_at DESC, id DESC"
        ))?;
        let v = st
            .query_map(params![parent_type, parent_id], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

/// Record the transcription job's state (and error, for `Failed`).
pub fn set_status(db: &Db, id: &str, status: JobStatus, error: Option<&str>) -> Result<()> {
    let n = db.with(|c| {
        Ok(c.prepare_cached(
            "UPDATE recordings SET transcript_status = ?2, transcript_error = ?3 WHERE id = ?1",
        )?
        .execute(params![id, status, error])?)
    })?;
    if n == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// Mark transcription `Running` and count the attempt (see `jobs::resume_pending`).
#[cfg(feature = "transcribe")]
pub(crate) fn start_transcript(db: &Db, id: &str) -> Result<()> {
    let n = db.with(|c| {
        Ok(c.prepare_cached(
            "UPDATE recordings SET transcript_status = 'running', transcript_error = NULL, \
             transcript_attempts = transcript_attempts + 1 WHERE id = ?1",
        )?
        .execute([id])?)
    })?;
    if n == 0 {
        return Err(not_found(id));
    }
    Ok(())
}

/// Store the finished transcript, mark Done, and index it for search, atomically.
pub fn set_transcript(db: &Db, id: &str, md: &str) -> Result<Recording> {
    db.tx(|t| {
        let rec = t
            .prepare_cached(&format!(
                "UPDATE recordings SET transcript_md = ?2, transcript_status = ?3, transcript_error = NULL \
                 WHERE id = ?1 RETURNING {COLS}"
            ))?
            .query_row(params![id, md, JobStatus::Done], row)
            .optional()?
            .ok_or_else(|| not_found(id))?;
        let (item_id, bucket_id) = parent_refs_conn(t, rec.parent_type, &rec.parent_id)?;
        search::index_conn(t, "recording", id, item_id.as_deref(), bucket_id.as_deref(), "Recording", md)?;
        Ok(rec)
    })
}

/// Where a recording's WAV is now. `audio_path` is stored absolute; when the data dir
/// has moved since (restored backup, renamed folder) that path is stale, so anything not
/// under `data_dir` resolves to `<data_dir>/audio/<file name>` instead.
pub fn resolve_audio_path(data_dir: &Path, audio_path: &str) -> PathBuf {
    let stored = Path::new(audio_path);
    if stored.starts_with(data_dir) {
        return stored.to_path_buf();
    }
    match stored.file_name() {
        Some(name) => data_dir.join("audio").join(name),
        None => stored.to_path_buf(),
    }
}

/// Remove the row (its search doc goes with it), then the audio file (see
/// [`resolve_audio_path`]); a missing file is fine.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    let audio = db.with(|c| {
        c.prepare_cached("DELETE FROM recordings WHERE id = ?1 RETURNING audio_path")?
            .query_row([id], |r| r.get::<_, String>(0))
            .optional()?
            .ok_or_else(|| not_found(id))
    })?;
    let audio = resolve_audio_path(db.data_dir(), &audio);
    match std::fs::remove_file(&audio) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r.io_ctx("remove", &audio),
    }
}

/// `(item_id, bucket_id)` for a search doc owned by this parent. Item-owned docs carry
/// the item's current bucket so bucket-scoped search finds them.
pub(crate) fn parent_refs_conn(
    c: &Connection,
    parent_type: ParentType,
    parent_id: &str,
) -> Result<(Option<String>, Option<String>)> {
    Ok(match parent_type {
        ParentType::Item => {
            let bucket: Option<String> = c
                .prepare_cached("SELECT bucket_id FROM items WHERE id = ?1")?
                .query_row([parent_id], |r| r.get(0))
                .optional()?
                .flatten();
            (Some(parent_id.to_string()), bucket)
        }
        ParentType::Bucket => (None, Some(parent_id.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EntityKind;
    use crate::models::{JobStatus, ParentType};
    use crate::repo::{search, test_bucket, test_db, test_item};

    #[test]
    fn recording_transcript_indexed_in_search() {
        let (_d, db) = test_db();
        let i1 = test_item(&db, "https://example.com/1").id;
        let b1 = test_bucket(&db, "B").id;
        let audio = db.data_dir().join("audio").join("r.wav");
        std::fs::write(&audio, b"RIFF").unwrap();
        let audio_s = audio.to_str().unwrap();

        let r = create(&db, ParentType::Item, &i1, audio_s, 1500).unwrap();
        assert_eq!(r.transcript_status, JobStatus::Pending);
        assert_eq!(r.duration_ms, 1500);
        assert_eq!(r.audio_path, audio_s);
        assert!(r.transcript_md.is_none());

        set_status(&db, &r.id, JobStatus::Running, None).unwrap();
        assert_eq!(
            get(&db, &r.id).unwrap().transcript_status,
            JobStatus::Running
        );
        set_status(&db, &r.id, JobStatus::Failed, Some("boom")).unwrap();
        let f = get(&db, &r.id).unwrap();
        assert_eq!(f.transcript_status, JobStatus::Failed);
        assert_eq!(f.transcript_error.as_deref(), Some("boom"));

        let t = set_transcript(&db, &r.id, "**[00:00]** remember the quokka").unwrap();
        assert_eq!(t.transcript_status, JobStatus::Done);
        assert!(t.transcript_error.is_none());
        assert_eq!(
            t.transcript_md.as_deref(),
            Some("**[00:00]** remember the quokka")
        );

        let hits = search::query(&db, "quokka", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].doc_type, "recording");
        assert_eq!(hits[0].doc_id, r.id);
        assert_eq!(hits[0].item_id.as_deref(), Some(i1.as_str()));
        assert_eq!(hits[0].bucket_id, None);
        assert_eq!(hits[0].title, "Recording");

        // Bucket-parented recordings index with bucket_id instead.
        let r2 = create(&db, ParentType::Bucket, &b1, "/nonexistent/x.wav", 10).unwrap();
        set_transcript(&db, &r2.id, "wombat").unwrap();
        let hits = search::query(&db, "wombat", 10).unwrap();
        assert_eq!(hits[0].bucket_id.as_deref(), Some(b1.as_str()));
        assert_eq!(hits[0].item_id, None);

        let l = list(&db, ParentType::Item, &i1).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].id, r.id);

        delete(&db, &r.id).unwrap();
        assert!(!audio.exists(), "audio file removed");
        assert!(
            search::query(&db, "quokka", 10).unwrap().is_empty(),
            "search doc removed"
        );
        assert!(matches!(
            get(&db, &r.id),
            Err(StowError::NotFound {
                kind: EntityKind::Recording,
                ..
            })
        ));
        // Missing audio file is ignored on delete.
        delete(&db, &r2.id).unwrap();
        assert!(list(&db, ParentType::Bucket, &b1).unwrap().is_empty());
    }

    #[test]
    fn item_recording_doc_carries_item_bucket() {
        use crate::repo::{buckets, items};
        let (_d, db) = test_db();
        let b = buckets::create(&db, "Pod", None).unwrap();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/r".into(),
                bucket_id: Some(b.id.clone()),
                title: None,
            },
        )
        .unwrap()
        .value;
        let r = create(&db, ParentType::Item, &it.id, "/nonexistent.wav", 1).unwrap();
        set_transcript(&db, &r.id, "numbat").unwrap();
        let hits = search::query(&db, "numbat", 10).unwrap();
        assert_eq!(hits[0].item_id.as_deref(), Some(it.id.as_str()));
        assert_eq!(hits[0].bucket_id.as_deref(), Some(b.id.as_str()));
    }

    #[test]
    fn recording_create_rejects_missing_parent() {
        let (_d, db) = test_db();
        let err = create(
            &db,
            ParentType::Item,
            "01NOPEXXXXXXXXXXXXXXXXXXXX",
            "/x.wav",
            1,
        )
        .unwrap_err();
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
        let err = create(
            &db,
            ParentType::Bucket,
            "01NOPEXXXXXXXXXXXXXXXXXXXX",
            "/x.wav",
            1,
        )
        .unwrap_err();
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
            .with(|c| Ok(c.query_row("SELECT count(*) FROM recordings", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn recording_delete_uses_data_dir_relative_path() {
        // The data dir was moved (restored backup, renamed folder): the stored absolute
        // path is stale, but the file sits under <data_dir>/audio by the same name.
        let (_d, db) = test_db();
        let it = test_item(&db, "https://example.com/moved");
        let actual = db.data_dir().join("audio").join("moved.wav");
        std::fs::write(&actual, b"RIFF").unwrap();
        let stale = "/Volumes/OldMac/Library/Application Support/Stow/audio/moved.wav";
        assert_eq!(resolve_audio_path(db.data_dir(), stale), actual);
        let inside = actual.to_string_lossy().into_owned();
        assert_eq!(
            resolve_audio_path(db.data_dir(), &inside),
            actual,
            "paths already under the data dir are kept"
        );

        let r = create(&db, ParentType::Item, &it.id, stale, 10).unwrap();
        assert_eq!(r.audio_path, stale, "stored as given");
        delete(&db, &r.id).unwrap();
        assert!(!actual.exists(), "audio file under the data dir removed");

        // The item cascade resolves the same way.
        let actual2 = db.data_dir().join("audio").join("moved2.wav");
        std::fs::write(&actual2, b"RIFF").unwrap();
        create(
            &db,
            ParentType::Item,
            &it.id,
            "/elsewhere/audio/moved2.wav",
            10,
        )
        .unwrap();
        crate::repo::items::delete(&db, &it.id).unwrap();
        assert!(!actual2.exists(), "cascade removed the audio file");
    }
}
