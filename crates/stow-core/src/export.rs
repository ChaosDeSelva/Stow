//! Export the library as Markdown folders or one JSON dump, and back up the database.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use rusqlite::types::ValueRef;

use crate::error::IoContext;
use crate::models::{Attachment, Bucket, Item, JobStatus, ParentType, Task};
use crate::repo::items::ItemFilter;
use crate::repo::{ai_runs, attachments, buckets, items, recordings, tasks};
use crate::{Db, Result};

/// Output format of [`export_library`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// One folder per bucket, one `.md` file per item, attachments copied alongside.
    Markdown,
    /// One JSON file with every table, ids preserved.
    Json,
}

/// What [`export_library`] wrote.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ExportReport {
    /// The export folder (Markdown) or file (JSON).
    pub path: PathBuf,
    /// Buckets exported (the Inbox is not counted).
    pub buckets: usize,
    /// Items exported.
    pub items: usize,
}

/// Tables in the JSON dump, parents before children.
const TABLES: &[&str] = &[
    "buckets",
    "items",
    "tasks",
    "reminders",
    "attachments",
    "recordings",
    "ai_runs",
    "settings",
];

/// Settings rows left out of the JSON dump: secrets that must not end up in a file the
/// user may share. (A [`backup_database`] copy keeps them; it is a private backup.)
const REDACTED_SETTINGS: &[&str] = &["extension_token"];

/// Export everything under `dest_dir` (created if missing).
///
/// - `Markdown`: `<dest>/Stow export <YYYY-MM-DD>/<bucket or Inbox>/<title-slug>-<id6>.md`
///   with YAML front matter (title, url, saved, kind, done, bucket) and Notes / Tasks /
///   Transcripts / AI notes sections; attachments are copied to `<title-slug>-<id6>/files/`.
///   Each bucket folder has a `README.md` with the bucket's description, notes, tasks, and
///   files. Archived buckets are included.
/// - `Json`: `<dest>/stow-export-<YYYY-MM-DD>.json` holding every row of every table
///   (ids preserved), except secret settings such as the extension token.
///
/// An existing export of the same name is never overwritten; a numeric suffix is added.
pub fn export_library(db: &Db, format: ExportFormat, dest_dir: &Path) -> Result<ExportReport> {
    std::fs::create_dir_all(dest_dir).io_ctx("create dir", dest_dir)?;
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    match format {
        ExportFormat::Markdown => {
            let root = unique_path(dest_dir, &format!("Stow export {date}"), "");
            export_markdown(db, &root)
        }
        ExportFormat::Json => {
            let file = unique_path(dest_dir, &format!("stow-export-{date}"), ".json");
            export_json(db, &file)
        }
    }
}

/// `dir/<stem><ext>`, or `dir/<stem> 2<ext>`, `... 3` ... for the first that does not exist.
fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = dir.join(format!("{stem}{ext}"));
    if !first.exists() {
        return first;
    }
    (2..10_000)
        .map(|n| dir.join(format!("{stem} {n}{ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| dir.join(format!("{stem} {}{ext}", crate::ids::new_id())))
}

/// Lowercase, alphanumerics kept, every other run of characters → one `-`, at most 60
/// bytes; `untitled` when nothing is left.
pub(crate) fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() {
            if out.len() + c.len_utf8() > 60 {
                break;
            }
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    if out.is_empty() {
        "untitled".into()
    } else {
        out.to_string()
    }
}

/// A display name usable as one path component: separators, reserved and control
/// characters → `-`, no leading/trailing dots or spaces, at most 80 characters.
pub(crate) fn folder_name(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .take(80)
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "Untitled".into()
    } else {
        trimmed.to_string()
    }
}

fn id6(id: &str) -> String {
    let start = id.len().saturating_sub(6);
    id.get(start..).unwrap_or(id).to_ascii_lowercase()
}

/// A YAML double-quoted scalar (JSON string syntax is valid YAML).
fn yaml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

fn write_file(path: &Path, contents: &str) -> Result<()> {
    std::fs::write(path, contents).io_ctx("write", path)
}

fn task_lines(list: &[Task]) -> String {
    list.iter()
        .map(|t| {
            let mark = if t.done_at.is_some() { 'x' } else { ' ' };
            format!("- [{mark}] {}", t.title)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Copy attachments into `files_dir`; returns Markdown list lines linking them relative
/// to `link_prefix`. A missing source is logged and skipped.
fn copy_files(list: &[Attachment], files_dir: &Path, link_prefix: &str) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for a in list {
        let name = folder_name(&a.filename);
        std::fs::create_dir_all(files_dir).io_ctx("create dir", files_dir)?;
        let dest = files_dir.join(&name);
        match std::fs::copy(&a.path, &dest) {
            Ok(_) => lines.push(format!("- [{name}](<{link_prefix}files/{name}>)")),
            Err(e) => tracing::warn!(attachment = %a.id, "export: could not copy {}: {e}", a.path),
        }
    }
    Ok(lines)
}

fn push_section(out: &mut String, heading: &str, body: &str) {
    if !body.trim().is_empty() {
        let _ = write!(out, "\n## {heading}\n\n{}\n", body.trim_end());
    }
}

/// Where each bucket's folder goes: `(bucket id or None for the Inbox, display name, dir)`.
/// Folder names are made unique case-insensitively (macOS volumes ignore case).
fn bucket_dirs(root: &Path, all: &[Bucket]) -> Vec<(Option<String>, String, PathBuf)> {
    let mut used: Vec<String> = vec!["inbox".into()];
    let mut dirs = vec![(None, "Inbox".to_string(), root.join("Inbox"))];
    for b in all {
        let mut name = folder_name(&b.name);
        if used.contains(&name.to_lowercase()) {
            name = format!("{name} ({})", id6(&b.id));
        }
        used.push(name.to_lowercase());
        dirs.push((Some(b.id.clone()), b.name.clone(), root.join(&name)));
    }
    dirs
}

fn write_bucket_readme(db: &Db, b: &Bucket, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).io_ctx("create dir", dir)?;
    let emoji = b
        .emoji
        .as_deref()
        .map(|e| format!("{e} "))
        .unwrap_or_default();
    let mut md = format!("# {emoji}{}\n", b.name);
    if !b.description_md.trim().is_empty() {
        let _ = write!(md, "\n{}\n", b.description_md.trim_end());
    }
    push_section(&mut md, "Notes", &b.notes_md);
    push_section(
        &mut md,
        "Tasks",
        &task_lines(&tasks::list(db, ParentType::Bucket, &b.id)?),
    );
    let files = copy_files(
        &attachments::list(db, ParentType::Bucket, &b.id)?,
        &dir.join("files"),
        "",
    )?;
    push_section(&mut md, "Files", &files.join("\n"));
    write_file(&dir.join("README.md"), &md)
}

fn write_item(db: &Db, it: &Item, bucket_name: &str, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).io_ctx("create dir", dir)?;
    let title = it.title.clone().unwrap_or_else(|| it.url.clone());
    let stem = format!("{}-{}", slug(&title), id6(&it.id));
    let mut md = format!(
        "---\ntitle: {}\nurl: {}\nsaved: {}\nkind: {}\ndone: {}\nbucket: {}\n---\n\n# {title}\n",
        yaml_str(&title),
        yaml_str(&it.url),
        yaml_str(&it.created_at),
        yaml_str(it.kind.as_str()),
        it.done_at.is_some(),
        yaml_str(bucket_name),
    );
    if let Some(d) = it.description.as_deref().filter(|d| !d.trim().is_empty()) {
        let _ = write!(md, "\n> {}\n", d.trim().replace('\n', "\n> "));
    }
    push_section(&mut md, "Notes", &it.notes_md);
    push_section(
        &mut md,
        "Tasks",
        &task_lines(&tasks::list(db, ParentType::Item, &it.id)?),
    );
    let mut recs = recordings::list(db, ParentType::Item, &it.id)?;
    recs.reverse();
    let transcripts = recs
        .iter()
        .filter_map(|r| {
            let t = r.transcript_md.as_deref()?.trim();
            (!t.is_empty()).then(|| format!("### Recording {}\n\n{t}", r.created_at))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    push_section(&mut md, "Transcripts", &transcripts);
    let mut runs = ai_runs::list(db, ParentType::Item, &it.id)?;
    runs.reverse();
    let notes = runs
        .iter()
        .filter(|r| r.status == JobStatus::Done)
        .filter_map(|r| {
            let out = r.output_md.as_deref()?.trim();
            (!out.is_empty()).then(|| format!("### {}\n\n{out}", r.action.as_str()))
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    push_section(&mut md, "AI notes", &notes);
    let files = copy_files(
        &attachments::list(db, ParentType::Item, &it.id)?,
        &dir.join(&stem).join("files"),
        &format!("{stem}/"),
    )?;
    push_section(&mut md, "Files", &files.join("\n"));
    write_file(&dir.join(format!("{stem}.md")), &md)
}

fn export_markdown(db: &Db, root: &Path) -> Result<ExportReport> {
    std::fs::create_dir_all(root).io_ctx("create dir", root)?;
    let all_items = items::list(db, ItemFilter::All, true)?;
    let mut all_buckets = buckets::list(db)?;
    // Items can sit in archived buckets, which `list` hides; export those too.
    for it in &all_items {
        if let Some(b) = &it.bucket_id {
            if !all_buckets.iter().any(|x| &x.id == b) {
                all_buckets.push(buckets::get(db, b)?);
            }
        }
    }
    let dirs = bucket_dirs(root, &all_buckets);
    let dir_of = |bucket_id: Option<&str>| {
        dirs.iter()
            .find(|(id, _, _)| id.as_deref() == bucket_id)
            .map_or_else(
                || ("Inbox".to_string(), root.join("Inbox")),
                |(_, n, d)| (n.clone(), d.clone()),
            )
    };
    for b in &all_buckets {
        write_bucket_readme(db, b, &dir_of(Some(&b.id)).1)?;
    }
    for it in &all_items {
        let (bucket_name, dir) = dir_of(it.bucket_id.as_deref());
        write_item(db, it, &bucket_name, &dir)?;
    }
    Ok(ExportReport {
        path: root.to_path_buf(),
        buckets: all_buckets.len(),
        items: all_items.len(),
    })
}

/// Lowercase hex.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

fn sql_to_json(v: ValueRef<'_>) -> serde_json::Value {
    match v {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(i) => i.into(),
        ValueRef::Real(f) => f.into(),
        ValueRef::Text(t) => String::from_utf8_lossy(t).into_owned().into(),
        ValueRef::Blob(b) => hex(b).into(),
    }
}

fn export_json(db: &Db, file: &Path) -> Result<ExportReport> {
    let (schema_version, tables) = db.with(|c| {
        let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let mut tables = serde_json::Map::new();
        for table in TABLES {
            let filter = if *table == "settings" {
                let keys: Vec<String> =
                    REDACTED_SETTINGS.iter().map(|k| format!("'{k}'")).collect();
                format!(" WHERE key NOT IN ({})", keys.join(", "))
            } else {
                String::new()
            };
            let mut st = c.prepare(&format!("SELECT * FROM {table}{filter} ORDER BY rowid"))?;
            let cols: Vec<String> = st.column_names().map(str::to_string).collect();
            let rows = st
                .query_map([], |r| {
                    let mut obj = serde_json::Map::new();
                    for (i, col) in cols.iter().enumerate() {
                        obj.insert(col.clone(), sql_to_json(r.get_ref(i)?));
                    }
                    Ok(serde_json::Value::Object(obj))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tables.insert((*table).to_string(), rows.into());
        }
        Ok((version, tables))
    })?;
    let count = |t: &str| tables.get(t).and_then(|v| v.as_array()).map_or(0, Vec::len);
    let report = ExportReport {
        path: file.to_path_buf(),
        buckets: count("buckets"),
        items: count("items"),
    };
    let doc = serde_json::json!({
        "format": "stow-export",
        "version": 1,
        "schema_version": schema_version,
        "exported_at": crate::ids::now(),
        "tables": tables,
    });
    let bytes = serde_json::to_vec_pretty(&doc)?;
    std::fs::write(file, bytes).io_ctx("write", file)?;
    Ok(report)
}

/// Copy the live database to `dest` with SQLite's online backup API (consistent even
/// while other connections write; WAL contents included). Creates missing parent
/// folders and replaces an existing file. Returns the size of the copy in bytes.
pub fn backup_database(db: &Db, dest: &Path) -> Result<u64> {
    if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).io_ctx("create dir", parent)?;
    }
    db.with(|c| {
        Ok(c.backup(
            rusqlite::MAIN_DB,
            dest,
            None::<fn(rusqlite::backup::Progress)>,
        )?)
    })?;
    Ok(std::fs::metadata(dest).io_ctx("stat", dest)?.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{AiAction, ParentType, Provider};
    use crate::repo::{ai_runs, attachments, buckets, items, recordings, tasks, test_db};

    fn today() -> String {
        chrono::Local::now().format("%Y-%m-%d").to_string()
    }

    /// A bucket with notes and tasks, one item in it with everything attached, one in the Inbox.
    fn seed(db: &Db, scratch: &Path) -> (String, String) {
        let b = buckets::create(db, "Blog Ideas", None).unwrap();
        buckets::update(
            db,
            &b.id,
            &buckets::BucketPatch {
                notes_md: Some("Bucket notes about **zebras**.".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let bt = tasks::create(db, ParentType::Bucket, &b.id, "Draft outline").unwrap();
        tasks::toggle(db, &bt.id).unwrap();
        tasks::create(db, ParentType::Bucket, &b.id, "Pick a title").unwrap();

        let it = items::create(
            db,
            items::NewItem {
                url: "https://example.com/post".into(),
                bucket_id: Some(b.id.clone()),
                title: Some("How to: Park Tabs!".into()),
            },
        )
        .unwrap()
        .value;
        items::set_notes(db, &it.id, "Item notes here.").unwrap();
        let t = tasks::create(db, ParentType::Item, &it.id, "Read it").unwrap();
        tasks::toggle(db, &t.id).unwrap();
        tasks::create(db, ParentType::Item, &it.id, "Share it").unwrap();
        let rec = recordings::create(db, ParentType::Item, &it.id, "/x.wav", 1000).unwrap();
        recordings::set_transcript(db, &rec.id, "**[00:00]** spoken words").unwrap();
        let run = ai_runs::create(
            db,
            ParentType::Item,
            &it.id,
            AiAction::SummarizePage,
            Provider::Anthropic,
            "m",
            "h",
        )
        .unwrap();
        ai_runs::complete(db, &run.id, "A short summary.").unwrap();
        let src = scratch.join("diagram.png");
        std::fs::write(&src, b"PNG").unwrap();
        attachments::add(db, ParentType::Item, &it.id, &src).unwrap();

        let inbox = items::create(
            db,
            items::NewItem {
                url: "https://example.org/inbox".into(),
                bucket_id: None,
                title: Some("Inbox thing".into()),
            },
        )
        .unwrap()
        .value;
        (it.id, inbox.id)
    }

    fn id6(id: &str) -> String {
        id[id.len() - 6..].to_ascii_lowercase()
    }

    #[test]
    fn export_markdown_writes_buckets_and_items() {
        let (_d, db) = test_db();
        let scratch = tempfile::tempdir().unwrap();
        let (item_id, inbox_id) = seed(&db, scratch.path());
        let dest = tempfile::tempdir().unwrap();
        let report = export_library(&db, ExportFormat::Markdown, dest.path()).unwrap();
        let root = dest.path().join(format!("Stow export {}", today()));
        assert_eq!(report.path, root);
        assert_eq!((report.buckets, report.items), (1, 2));

        let readme = std::fs::read_to_string(root.join("Blog Ideas/README.md")).unwrap();
        assert!(readme.starts_with("# Blog Ideas"), "{readme}");
        assert!(
            readme.contains("Bucket notes about **zebras**."),
            "{readme}"
        );
        assert!(readme.contains("- [x] Draft outline"), "{readme}");
        assert!(readme.contains("- [ ] Pick a title"), "{readme}");

        let stem = format!("how-to-park-tabs-{}", id6(&item_id));
        let md = std::fs::read_to_string(root.join(format!("Blog Ideas/{stem}.md"))).unwrap();
        assert!(md.starts_with("---\n"), "{md}");
        for needle in [
            "url: \"https://example.com/post\"",
            "kind: \"article\"",
            "done: false",
            "bucket: \"Blog Ideas\"",
            "saved: \"",
            "# How to: Park Tabs!",
            "## Notes\n\nItem notes here.",
            "## Tasks\n\n- [x] Read it\n- [ ] Share it",
            "## Transcripts",
            "spoken words",
            "## AI notes",
            "A short summary.",
            "files/diagram.png",
        ] {
            assert!(md.contains(needle), "missing {needle:?} in:\n{md}");
        }
        assert_eq!(
            std::fs::read(root.join(format!("Blog Ideas/{stem}/files/diagram.png"))).unwrap(),
            b"PNG"
        );
        let inbox_file = root.join(format!("Inbox/inbox-thing-{}.md", id6(&inbox_id)));
        let inbox_text = std::fs::read_to_string(inbox_file).unwrap();
        assert!(inbox_text.contains("bucket: \"Inbox\""), "{inbox_text}");

        // A second export the same day goes to a new folder instead of overwriting.
        let again = export_library(&db, ExportFormat::Markdown, dest.path()).unwrap();
        assert_ne!(again.path, root);
        assert!(again.path.is_dir());
    }

    #[test]
    fn export_names_are_filesystem_safe() {
        assert_eq!(slug("How to: Park Tabs!"), "how-to-park-tabs");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("   "), "untitled");
        assert_eq!(slug("Ünïcödé café"), "ünïcödé-café");
        assert!(slug(&"x".repeat(500)).len() <= 60);
        assert_eq!(folder_name("a/b: c?"), "a-b- c-");
        assert_eq!(folder_name(".."), "Untitled");
        assert_eq!(folder_name("..\\x"), "-x");
    }

    #[test]
    fn export_json_roundtrips_counts() {
        let (_d, db) = test_db();
        let scratch = tempfile::tempdir().unwrap();
        let (item_id, _) = seed(&db, scratch.path());
        crate::repo::settings::set(&db, "extension_token", "secret-token-123").unwrap();
        crate::repo::settings::set(&db, "theme", "dark").unwrap();
        let dest = tempfile::tempdir().unwrap();
        let report = export_library(&db, ExportFormat::Json, dest.path()).unwrap();
        let raw = std::fs::read_to_string(&report.path).unwrap();
        assert!(
            !raw.contains("secret-token-123"),
            "extension token exported"
        );
        assert_eq!(
            report.path,
            dest.path().join(format!("stow-export-{}.json", today()))
        );
        assert_eq!((report.buckets, report.items), (1, 2));
        let v: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&report.path).unwrap()).unwrap();
        assert_eq!(v["format"], "stow-export");
        assert_eq!(v["schema_version"], 2);
        let count = |table: &str| -> usize {
            db.with(|c| {
                Ok(
                    c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| {
                        r.get::<_, i64>(0)
                    })?,
                )
            })
            .map(|n| usize::try_from(n).unwrap())
            .unwrap()
        };
        for table in [
            "buckets",
            "items",
            "tasks",
            "reminders",
            "attachments",
            "recordings",
            "ai_runs",
            "settings",
        ] {
            let rows = v["tables"][table]
                .as_array()
                .unwrap_or_else(|| panic!("{table}"));
            let redacted = usize::from(table == "settings");
            assert_eq!(rows.len(), count(table) - redacted, "{table}");
        }
        let items = v["tables"]["items"].as_array().unwrap();
        let it = items.iter().find(|r| r["id"] == item_id.as_str()).unwrap();
        assert_eq!(it["url"], "https://example.com/post");
        assert_eq!(it["notes_md"], "Item notes here.");
        assert_eq!(it["snapshot_attempts"], 0);
    }

    #[test]
    fn backup_database_is_openable() {
        let (_d, db) = test_db();
        let scratch = tempfile::tempdir().unwrap();
        seed(&db, scratch.path());
        let dest = tempfile::tempdir().unwrap();
        let file = dest.path().join("copy").join("stow.db");
        let bytes = backup_database(&db, &file).unwrap();
        assert!(bytes > 0);
        assert_eq!(bytes, std::fs::metadata(&file).unwrap().len());
        let copy = Db::open(&dest.path().join("copy")).unwrap();
        assert_eq!(
            items::list(&copy, items::ItemFilter::All, true)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            crate::repo::search::query(&copy, "zebras", 10)
                .unwrap()
                .len(),
            1,
            "fts index came along"
        );
    }
}
