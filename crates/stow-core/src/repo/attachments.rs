//! Files attached to a bucket or item; bytes live under `<data_dir>/attachments/<id>/<filename>`.

use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension, Row};

use crate::error::{EntityKind, IoContext};
use crate::ids::{new_id, now};
use crate::models::{Attachment, ParentType};
use crate::{Db, Result, StowError};

const COLS: &str = "id, parent_type, parent_id, filename, path, mime, size_bytes, created_at";

fn row(r: &Row) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: r.get(0)?,
        parent_type: r.get(1)?,
        parent_id: r.get(2)?,
        filename: r.get(3)?,
        path: r.get(4)?,
        mime: r.get(5)?,
        size_bytes: r.get(6)?,
        created_at: r.get(7)?,
    })
}

fn dir_for(db: &Db, id: &str) -> PathBuf {
    db.data_dir().join("attachments").join(id)
}

/// Copy `src` into the data dir and record it. The source file is left in place. The
/// parent must exist (`NotFound` otherwise, and nothing is left under `attachments/`).
pub fn add(db: &Db, parent_type: ParentType, parent_id: &str, src: &Path) -> Result<Attachment> {
    let filename = src
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| StowError::invalid("src_path", src.display().to_string()))?
        .to_string();
    let id = new_id();
    let dir = dir_for(db, &id);
    std::fs::create_dir_all(&dir).io_ctx("create dir", &dir)?;
    let dest = dir.join(&filename);
    let copied = match std::fs::copy(src, &dest).io_ctx("copy", src) {
        Ok(n) => n,
        Err(e) => {
            remove_dir_logged(&dir);
            return Err(e);
        }
    };
    let mime = mime_guess::from_path(&filename)
        .first_raw()
        .unwrap_or("application/octet-stream");
    let size = i64::try_from(copied).unwrap_or(i64::MAX);
    let inserted = db.tx(|c| {
        crate::repo::require_parent(c, parent_type, parent_id)?;
        Ok(c.prepare_cached(&format!(
            "INSERT INTO attachments (id, parent_type, parent_id, filename, path, mime, size_bytes, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) RETURNING {COLS}"
        ))?
        .query_row(
            params![id, parent_type, parent_id, filename, dest.to_string_lossy(), mime, size, now()],
            row,
        )?)
    });
    if inserted.is_err() {
        remove_dir_logged(&dir);
    }
    inserted
}

/// Best-effort cleanup of a half-made attachment dir; failures are logged.
fn remove_dir_logged(dir: &Path) {
    if let Err(e) = std::fs::remove_dir_all(dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(dir = %dir.display(), "could not clean up attachment dir: {e}");
        }
    }
}

/// One attachment by id.
pub fn get(db: &Db, id: &str) -> Result<Attachment> {
    db.with(|c| {
        c.prepare_cached(&format!("SELECT {COLS} FROM attachments WHERE id = ?1"))?
            .query_row([id], row)
            .optional()?
            .ok_or_else(|| StowError::not_found(EntityKind::Attachment, id))
    })
}

/// The parent's attachments, oldest first.
pub fn list(db: &Db, parent_type: ParentType, parent_id: &str) -> Result<Vec<Attachment>> {
    db.with(|c| {
        let mut st = c.prepare_cached(&format!(
            "SELECT {COLS} FROM attachments WHERE parent_type = ?1 AND parent_id = ?2 ORDER BY created_at, id"
        ))?;
        let v = st
            .query_map(params![parent_type, parent_id], row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(v)
    })
}

/// Remove the row, then (after it is gone) the `attachments/<id>` directory; a missing
/// directory is fine.
pub fn delete(db: &Db, id: &str) -> Result<()> {
    db.with(|c| {
        let n = c
            .prepare_cached("DELETE FROM attachments WHERE id = ?1")?
            .execute([id])?;
        if n == 0 {
            return Err(StowError::not_found(EntityKind::Attachment, id));
        }
        Ok(())
    })?;
    let dir = dir_for(db, id);
    match std::fs::remove_dir_all(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        r => r.io_ctx("remove dir", &dir),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ParentType;
    use crate::repo::{test_db, test_item};

    #[test]
    fn attachment_add_copies_file_and_delete_removes() {
        let (dir, db) = test_db();
        let i1 = test_item(&db, "https://example.com/1").id;
        let src = dir.path().join("notes.txt");
        std::fs::write(&src, b"hello world").unwrap();

        let a = add(&db, ParentType::Item, &i1, &src).unwrap();
        assert_eq!(a.filename, "notes.txt");
        assert_eq!(a.mime, "text/plain");
        assert_eq!(a.size_bytes, 11);
        assert_eq!(a.parent_type, ParentType::Item);
        assert_eq!(a.parent_id, i1);
        let expected = db
            .data_dir()
            .join("attachments")
            .join(&a.id)
            .join("notes.txt");
        assert_eq!(std::path::Path::new(&a.path), expected);
        assert_eq!(std::fs::read(&a.path).unwrap(), b"hello world");
        assert!(src.is_file(), "source is copied, not moved");

        let bin = dir.path().join("blob.weirdext");
        std::fs::write(&bin, [0u8; 3]).unwrap();
        let b = add(&db, ParentType::Item, &i1, &bin).unwrap();
        assert_eq!(b.mime, "application/octet-stream");
        assert_eq!(b.size_bytes, 3);

        let l = list(&db, ParentType::Item, &i1).unwrap();
        assert_eq!(
            l.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(),
            vec![a.id.as_str(), b.id.as_str()]
        );
        assert!(list(&db, ParentType::Bucket, &i1).unwrap().is_empty());
        assert_eq!(get(&db, &a.id).unwrap(), a);

        delete(&db, &a.id).unwrap();
        assert!(
            !expected.parent().unwrap().exists(),
            "attachment dir removed"
        );
        assert!(matches!(
            get(&db, &a.id),
            Err(StowError::NotFound {
                kind: EntityKind::Attachment,
                ..
            })
        ));
        assert_eq!(list(&db, ParentType::Item, &i1).unwrap().len(), 1);

        // Deleting when the directory is already gone is not an error.
        std::fs::remove_dir_all(db.data_dir().join("attachments").join(&b.id)).unwrap();
        delete(&db, &b.id).unwrap();
        assert!(list(&db, ParentType::Item, &i1).unwrap().is_empty());
    }

    fn attachment_dirs(db: &Db) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(db.data_dir().join("attachments"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    }

    #[test]
    fn attachment_add_rejects_directories_and_missing_files_without_leftovers() {
        let (dir, db) = test_db();
        let i1 = test_item(&db, "https://example.com/1").id;
        let folder = dir.path().join("a-folder");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        std::fs::write(folder.join("inner").join("f.txt"), b"x").unwrap();

        let err = add(&db, ParentType::Item, &i1, &folder).expect_err("a directory");
        assert!(matches!(err, StowError::Io { op: "copy", .. }), "{err:?}");
        assert_eq!(err.code(), "io");
        assert!(
            list(&db, ParentType::Item, &i1).unwrap().is_empty(),
            "no row"
        );
        assert!(
            attachment_dirs(&db).is_empty(),
            "no half-made attachment dir"
        );
        assert!(
            folder.join("inner").join("f.txt").is_file(),
            "source untouched"
        );

        let missing = dir.path().join("nope.txt");
        let err = add(&db, ParentType::Item, &i1, &missing).expect_err("missing file");
        assert!(
            matches!(&err, StowError::Io { op: "copy", path, .. } if path == &missing),
            "{err:?}"
        );
        assert!(attachment_dirs(&db).is_empty());

        // Paths with no usable file name are invalid input, not io errors.
        for bad in [std::path::Path::new("/"), std::path::Path::new("")] {
            let err = add(&db, ParentType::Item, &i1, bad).expect_err(&bad.display().to_string());
            assert!(
                matches!(
                    err,
                    StowError::InvalidInput {
                        field: "src_path",
                        ..
                    }
                ),
                "{err:?}"
            );
        }
        assert!(attachment_dirs(&db).is_empty());
        assert!(list(&db, ParentType::Item, &i1).unwrap().is_empty());
    }

    #[test]
    fn attachment_add_rejects_missing_parent() {
        let (dir, db) = test_db();
        let src = dir.path().join("orphan.txt");
        std::fs::write(&src, b"x").unwrap();
        for (pt, kind) in [
            (ParentType::Item, EntityKind::Item),
            (ParentType::Bucket, EntityKind::Bucket),
        ] {
            let err = add(&db, pt, "01NOPEXXXXXXXXXXXXXXXXXXXX", &src).unwrap_err();
            assert!(
                matches!(&err, StowError::NotFound { kind: k, .. } if *k == kind),
                "{err:?}"
            );
        }
        assert!(attachment_dirs(&db).is_empty(), "no copied dir left behind");
        let n: i64 = db
            .with(|c| Ok(c.query_row("SELECT count(*) FROM attachments", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(n, 0);
    }
}
