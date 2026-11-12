//! What `get_item` and the `stow://` resources show: an item or bucket with everything
//! hanging off it, as JSON (serde) or Markdown.

use std::fmt::Write as _;

use serde::Serialize;
use stow_core::error::EntityKind;
use stow_core::fetch::snapshot::read_snapshot_md;
use stow_core::models::{Bucket, Item, JobStatus, ParentType, TargetType};
use stow_core::repo::items::ItemFilter;
use stow_core::repo::{ai_runs, buckets, items, recordings, reminders, tasks};
use stow_core::{ids, Db, Result, StowError};

/// Longest readable text returned by `get_item`, in characters; more is cut with a marker.
pub const TEXT_LIMIT: usize = 60_000;

/// Resolve a bucket argument: `None`, empty, or `inbox` (any case) → the Inbox (`None`);
/// a ULID of an existing bucket → that bucket; otherwise a case-insensitive name.
/// `NotFound { kind: Bucket }` when nothing matches.
pub(crate) fn resolve_bucket(db: &Db, spec: Option<&str>) -> Result<Option<String>> {
    let Some(spec) = spec
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("inbox"))
    else {
        return Ok(None);
    };
    if ids::is_ulid(spec) {
        // Ids are stored upper-case; accept either case like `items::find_by_prefix` does.
        match buckets::get(db, &spec.to_ascii_uppercase()) {
            Ok(b) => return Ok(Some(b.id)),
            Err(e) if !e.is_not_found() => return Err(e),
            Err(_) => {}
        }
    }
    buckets::find_by_name(db, spec)?
        .map(|b| Some(b.id))
        .ok_or_else(|| StowError::not_found(EntityKind::Bucket, spec))
}

/// `{id, name}` of an item's bucket.
#[derive(Debug, Clone, Serialize)]
pub struct BucketRef {
    /// ULID.
    pub id: String,
    /// Display name.
    pub name: String,
}

impl BucketRef {
    fn load(db: &Db, id: Option<&str>) -> Result<Option<Self>> {
        id.map(|id| {
            buckets::get(db, id).map(|b| BucketRef {
                id: b.id,
                name: b.name,
            })
        })
        .transpose()
    }
}

/// A task as the client sees it.
#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    /// ULID.
    pub id: String,
    /// Task text.
    pub title: String,
    /// Ticked off.
    pub done: bool,
}

/// A reminder as the client sees it.
#[derive(Debug, Clone, Serialize)]
pub struct ReminderView {
    /// ULID.
    pub id: String,
    /// Effective time (snoozed time when set, else the scheduled one), RFC 3339 UTC.
    pub at: String,
    /// Optional note.
    pub message: Option<String>,
    /// Already fired.
    pub fired: bool,
}

/// A recording's transcript.
#[derive(Debug, Clone, Serialize)]
pub struct TranscriptView {
    /// Recording ULID.
    pub recording_id: String,
    /// When it was recorded, RFC 3339 UTC.
    pub created_at: String,
    /// Length in milliseconds.
    pub duration_ms: i64,
    /// `pending | running | done | failed`.
    pub status: JobStatus,
    /// The transcript, when done.
    pub transcript_md: Option<String>,
}

/// A finished or failed AI run.
#[derive(Debug, Clone, Serialize)]
pub struct AiRunView {
    /// ULID.
    pub id: String,
    /// What was asked (`summarize_page`, …).
    pub action: String,
    /// Provider and model.
    pub provider: String,
    /// Model id.
    pub model: String,
    /// Job state.
    pub status: JobStatus,
    /// The reply, when done.
    pub output_md: Option<String>,
    /// RFC 3339 UTC.
    pub created_at: String,
}

/// One row of `list_items`.
#[derive(Debug, Clone, Serialize)]
pub struct ItemRow {
    /// ULID.
    pub id: String,
    /// Title (fetched, typed, or the host).
    pub title: Option<String>,
    /// URL as saved.
    pub url: String,
    /// `video | article | other`.
    pub kind: String,
    /// Bucket name, or `Inbox`.
    pub bucket: String,
    /// Bucket id, `None` for the Inbox.
    pub bucket_id: Option<String>,
    /// When it was saved, RFC 3339 UTC.
    pub saved_at: String,
    /// Marked done (the app calls this Archive).
    pub archived: bool,
    /// Readable-snapshot job state.
    pub snapshot_status: JobStatus,
}

impl ItemRow {
    pub(crate) fn new(it: Item, bucket_name: &str) -> Self {
        ItemRow {
            id: it.id,
            title: it.title,
            url: it.url,
            kind: it.kind.as_str().to_string(),
            bucket: bucket_name.to_string(),
            bucket_id: it.bucket_id,
            saved_at: it.created_at,
            archived: it.done_at.is_some(),
            snapshot_status: it.snapshot_status,
        }
    }
}

/// Everything about one item, for `get_item` and `stow://item/{id}`.
#[derive(Debug, Clone, Serialize)]
pub struct ItemView {
    /// ULID.
    pub id: String,
    /// Title.
    pub title: Option<String>,
    /// URL as saved.
    pub url: String,
    /// Normalized URL (the dedupe key).
    pub canonical_url: String,
    /// `video | article | other`.
    pub kind: String,
    /// Owning bucket, `None` for the Inbox.
    pub bucket: Option<BucketRef>,
    /// Fetched site name.
    pub site_name: Option<String>,
    /// Fetched description.
    pub description: Option<String>,
    /// When it was saved, RFC 3339 UTC.
    pub saved_at: String,
    /// Last change, RFC 3339 UTC.
    pub updated_at: String,
    /// Marked done (the app calls this Archive).
    pub archived: bool,
    /// Readable-snapshot job state.
    pub snapshot_status: JobStatus,
    /// Why the snapshot failed.
    pub snapshot_error: Option<String>,
    /// User notes (Markdown).
    pub notes_md: String,
    /// Checklist.
    pub tasks: Vec<TaskView>,
    /// Reminders on the item.
    pub reminders: Vec<ReminderView>,
    /// Readable text (Markdown), `None` when not requested or never fetched.
    pub text: Option<String>,
    /// `text` was cut at [`TEXT_LIMIT`] characters.
    pub text_truncated: bool,
    /// Voice memos, oldest first.
    pub transcripts: Vec<TranscriptView>,
    /// AI runs, oldest first.
    pub ai_runs: Vec<AiRunView>,
}

impl ItemView {
    /// Load the item and its children; `include_text` also reads the snapshot file.
    pub fn load(db: &Db, id: &str, include_text: bool) -> Result<Self> {
        let it = items::get(db, id)?;
        let bucket = BucketRef::load(db, it.bucket_id.as_deref())?;
        let (text, text_truncated) = if include_text {
            match read_snapshot_md(db.data_dir(), &it.id)? {
                Some(md) => {
                    let (t, cut) = truncate(&md);
                    (Some(t), cut)
                }
                None => (None, false),
            }
        } else {
            (None, false)
        };
        let mut recs = recordings::list(db, ParentType::Item, &it.id)?;
        recs.reverse();
        let mut runs = ai_runs::list(db, ParentType::Item, &it.id)?;
        runs.reverse();
        Ok(ItemView {
            tasks: task_views(db, ParentType::Item, &it.id)?,
            reminders: reminder_views(db, TargetType::Item, &it.id)?,
            id: it.id,
            title: it.title,
            url: it.url,
            canonical_url: it.canonical_url,
            kind: it.kind.as_str().to_string(),
            bucket,
            site_name: it.site_name,
            description: it.description,
            saved_at: it.created_at,
            updated_at: it.updated_at,
            archived: it.done_at.is_some(),
            snapshot_status: it.snapshot_status,
            snapshot_error: it.snapshot_error,
            notes_md: it.notes_md,
            text,
            text_truncated,
            transcripts: recs
                .into_iter()
                .map(|r| TranscriptView {
                    recording_id: r.id,
                    created_at: r.created_at,
                    duration_ms: r.duration_ms,
                    status: r.transcript_status,
                    transcript_md: r.transcript_md,
                })
                .collect(),
            ai_runs: runs
                .into_iter()
                .map(|r| AiRunView {
                    id: r.id,
                    action: r.action.as_str().to_string(),
                    provider: r.provider.as_str().to_string(),
                    model: r.model,
                    status: r.status,
                    output_md: r.output_md,
                    created_at: r.created_at,
                })
                .collect(),
        })
    }

    /// The item as a Markdown document.
    pub fn to_markdown(&self) -> String {
        let title = self.title.as_deref().unwrap_or(&self.url);
        let mut md = format!("# {title}\n\n- URL: {}\n- Id: {}\n", self.url, self.id);
        let _ = writeln!(
            md,
            "- Bucket: {}",
            self.bucket.as_ref().map_or("Inbox", |b| b.name.as_str())
        );
        let _ = writeln!(md, "- Kind: {}", self.kind);
        let _ = writeln!(md, "- Saved: {}", self.saved_at);
        if self.archived {
            md.push_str("- Archived: yes\n");
        }
        if let Some(site) = self.site_name.as_deref().filter(|s| !s.trim().is_empty()) {
            let _ = writeln!(md, "- Site: {site}");
        }
        if let Some(d) = self.description.as_deref().filter(|d| !d.trim().is_empty()) {
            let _ = write!(md, "\n> {}\n", d.trim().replace('\n', "\n> "));
        }
        section(&mut md, "Notes", &self.notes_md);
        section(&mut md, "Tasks", &task_lines(&self.tasks));
        let reminders = self
            .reminders
            .iter()
            .map(|r| {
                let fired = if r.fired { " (fired)" } else { "" };
                match &r.message {
                    Some(m) => format!("- {}{fired}: {m}", r.at),
                    None => format!("- {}{fired}", r.at),
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        section(&mut md, "Reminders", &reminders);
        let transcripts = self
            .transcripts
            .iter()
            .filter_map(|t| {
                let text = t.transcript_md.as_deref()?.trim();
                (!text.is_empty()).then(|| format!("### Recording {}\n\n{text}", t.created_at))
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        section(&mut md, "Transcripts", &transcripts);
        let notes = self
            .ai_runs
            .iter()
            .filter(|r| r.status == JobStatus::Done)
            .filter_map(|r| {
                let out = r.output_md.as_deref()?.trim();
                (!out.is_empty()).then(|| format!("### {}\n\n{out}", r.action))
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        section(&mut md, "AI notes", &notes);
        if let Some(text) = &self.text {
            section(&mut md, "Text", text);
        } else {
            let why = match self.snapshot_status {
                JobStatus::Failed => format!(
                    "not available ({})",
                    self.snapshot_error.as_deref().unwrap_or("fetch failed")
                ),
                JobStatus::Pending | JobStatus::Running => "not fetched yet".to_string(),
                JobStatus::None | JobStatus::Done => "not available".to_string(),
            };
            section(&mut md, "Text", &format!("_{why}_"));
        }
        md
    }
}

/// A bucket with its own notes, tasks, and items, for `stow://bucket/{id}`.
#[derive(Debug, Clone, Serialize)]
pub struct BucketView {
    /// The bucket row.
    #[serde(flatten)]
    pub bucket: Bucket,
    /// Checklist.
    pub tasks: Vec<TaskView>,
    /// Reminders on the bucket.
    pub reminders: Vec<ReminderView>,
    /// Items in the bucket, newest first (archived ones included).
    pub items: Vec<ItemRow>,
}

impl BucketView {
    /// Load the bucket (archived ones too) with its tasks, reminders, and items.
    pub fn load(db: &Db, id: &str) -> Result<Self> {
        let bucket = buckets::get(db, id)?;
        let list = items::list(db, ItemFilter::Bucket(bucket.id.clone()), true)?;
        Ok(BucketView {
            tasks: task_views(db, ParentType::Bucket, &bucket.id)?,
            reminders: reminder_views(db, TargetType::Bucket, &bucket.id)?,
            items: list
                .into_iter()
                .map(|it| ItemRow::new(it, &bucket.name))
                .collect(),
            bucket,
        })
    }

    /// The bucket as a Markdown document; items link to their `stow://item/{id}` resource.
    pub fn to_markdown(&self) -> String {
        let b = &self.bucket;
        let emoji = b
            .emoji
            .as_deref()
            .map(|e| format!("{e} "))
            .unwrap_or_default();
        let mut md = format!("# {emoji}{}\n\n- Id: {}\n", b.name, b.id);
        if b.archived_at.is_some() {
            md.push_str("- Archived: yes\n");
        }
        if !b.description_md.trim().is_empty() {
            let _ = write!(md, "\n{}\n", b.description_md.trim_end());
        }
        section(&mut md, "Notes", &b.notes_md);
        section(&mut md, "Tasks", &task_lines(&self.tasks));
        let items = self
            .items
            .iter()
            .map(|it| {
                let done = if it.archived { " (archived)" } else { "" };
                format!(
                    "- {}{done} — {} — stow://item/{}",
                    it.title.as_deref().unwrap_or(&it.url),
                    it.url,
                    it.id
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        section(
            &mut md,
            &format!("Items ({})", self.items.len()),
            if items.is_empty() { "_none_" } else { &items },
        );
        md
    }
}

fn task_views(db: &Db, parent: ParentType, id: &str) -> Result<Vec<TaskView>> {
    Ok(tasks::list(db, parent, id)?
        .into_iter()
        .map(|t| TaskView {
            id: t.id,
            title: t.title,
            done: t.done_at.is_some(),
        })
        .collect())
}

fn reminder_views(db: &Db, target: TargetType, id: &str) -> Result<Vec<ReminderView>> {
    Ok(reminders::list_for(db, target, id)?
        .into_iter()
        .map(|r| ReminderView {
            id: r.id,
            at: r.snoozed_until.unwrap_or(r.fire_at),
            message: r.message,
            fired: r.fired_at.is_some(),
        })
        .collect())
}

fn task_lines(list: &[TaskView]) -> String {
    list.iter()
        .map(|t| format!("- [{}] {}", if t.done { 'x' } else { ' ' }, t.title))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `## heading` plus body, unless the body is blank.
fn section(out: &mut String, heading: &str, body: &str) {
    if !body.trim().is_empty() {
        let _ = write!(out, "\n## {heading}\n\n{}\n", body.trim_end());
    }
}

/// The first [`TEXT_LIMIT`] characters plus a marker, and whether anything was cut.
pub(crate) fn truncate(text: &str) -> (String, bool) {
    let total = text.chars().count();
    if total <= TEXT_LIMIT {
        return (text.to_string(), false);
    }
    let mut out: String = text.chars().take(TEXT_LIMIT).collect();
    let _ = write!(
        out,
        "\n\n[… truncated: {TEXT_LIMIT} of {total} characters shown]"
    );
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_cuts_on_char_boundary_with_marker() {
        let short = "héllo";
        assert_eq!(truncate(short), (short.to_string(), false));
        let long: String = "é".repeat(TEXT_LIMIT + 5);
        let (t, cut) = truncate(&long);
        assert!(cut);
        assert!(t.starts_with(&"é".repeat(TEXT_LIMIT)));
        assert!(t.ends_with(&format!(
            "{TEXT_LIMIT} of {} characters shown]",
            TEXT_LIMIT + 5
        )));
    }

    #[test]
    fn resolve_bucket_by_id_name_and_inbox() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(dir.path()).unwrap();
        let b = buckets::create(&db, "Blog Ideas", None).unwrap();
        for inbox in [None, Some(""), Some("  "), Some("inbox"), Some("INBOX")] {
            assert_eq!(resolve_bucket(&db, inbox).unwrap(), None, "{inbox:?}");
        }
        assert_eq!(
            resolve_bucket(&db, Some(&b.id)).unwrap(),
            Some(b.id.clone())
        );
        assert_eq!(
            resolve_bucket(&db, Some(&b.id.to_ascii_lowercase())).unwrap(),
            Some(b.id.clone())
        );
        assert_eq!(
            resolve_bucket(&db, Some(" blog ideas ")).unwrap(),
            Some(b.id.clone())
        );
        // A name that happens to look like a ULID still resolves by name.
        let odd = buckets::create(&db, "01ARZ3NDEKTSV4RRFFQ69G5FAV", None).unwrap();
        assert_eq!(
            resolve_bucket(&db, Some("01ARZ3NDEKTSV4RRFFQ69G5FAV")).unwrap(),
            Some(odd.id)
        );
        let err = resolve_bucket(&db, Some("Nope")).unwrap_err();
        assert!(
            matches!(err, StowError::NotFound { kind: EntityKind::Bucket, ref id } if id == "Nope")
        );
    }
}
