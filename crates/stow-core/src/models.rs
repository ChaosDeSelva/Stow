//! Row types shared by core, CLI, and the Tauri app. Field names match SQL columns.

use serde::{Deserialize, Serialize};

macro_rules! str_enum {
    ($(#[$meta:meta])* $name:ident ($field:literal) { $($(#[$vmeta:meta])* $variant:ident = $s:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name { $($(#[$vmeta])* #[serde(rename = $s)] $variant),+ }

        impl $name {
            /// The string stored in SQLite and sent over JSON.
            pub fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $s),+ }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = crate::StowError;
            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                match s {
                    $($s => Ok(Self::$variant),)+
                    other => Err(crate::StowError::invalid($field, other)),
                }
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(v: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
                let s = <String as rusqlite::types::FromSql>::column_result(v)?;
                s.parse().map_err(|e| rusqlite::types::FromSqlError::Other(Box::new(e)))
            }
        }

        impl rusqlite::types::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(self.as_str().into())
            }
        }
    };
}

str_enum!(
    /// What a saved link points at.
    ItemKind("kind") {
        /// `YouTube`, Vimeo, Loom, Twitch.
        Video = "video",
        /// Any other web page.
        Article = "article",
        /// Reserved for non-page links.
        Other = "other",
    }
);
str_enum!(
    /// Lifecycle of a background job recorded on a row.
    JobStatus("status") {
        /// Never queued.
        None = "none",
        /// Queued.
        Pending = "pending",
        /// In flight.
        Running = "running",
        /// Finished successfully.
        Done = "done",
        /// Finished with an error (see the row's `*_error`).
        Failed = "failed",
    }
);
str_enum!(
    /// Owner of a task, attachment, recording, or AI run.
    ParentType("parent_type") {
        /// A bucket.
        Bucket = "bucket",
        /// An item.
        Item = "item",
    }
);
str_enum!(
    /// What a reminder is attached to.
    TargetType("target_type") {
        /// A bucket.
        Bucket = "bucket",
        /// An item.
        Item = "item",
        /// A task.
        Task = "task",
    }
);
str_enum!(
    /// What an AI run does.
    AiAction("action") {
        /// Bullet summary of the page snapshot.
        SummarizePage = "summarize_page",
        /// Summary of the user's notes.
        SummarizeNotes = "summarize_notes",
        /// Summary of the latest transcript.
        SummarizeRecording = "summarize_recording",
        /// A checklist of next tasks.
        SuggestTasks = "suggest_tasks",
        /// Title/description/site as JSON.
        ExtractMetadata = "extract_metadata",
    }
);
str_enum!(
    /// AI provider (`anthropic` | `openai`); stored as TEXT in `ai_runs.provider`.
    Provider("ai_provider") {
        /// Anthropic Messages API.
        Anthropic = "anthropic",
        /// `OpenAI` chat completions API.
        OpenAi = "openai",
    }
);

/// An intent-named pile of items.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bucket {
    /// ULID.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Optional emoji shown before the name.
    pub emoji: Option<String>,
    /// What the bucket is for (Markdown).
    pub description_md: String,
    /// Free-form notes (Markdown).
    pub notes_md: String,
    /// Sidebar order, 1-based.
    pub position: i64,
    /// RFC 3339 UTC.
    pub created_at: String,
    /// RFC 3339 UTC.
    pub updated_at: String,
    /// Set when archived (hidden from lists).
    pub archived_at: Option<String>,
}

/// A saved link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// ULID.
    pub id: String,
    /// Owning bucket; `None` = Inbox.
    pub bucket_id: Option<String>,
    /// URL as captured.
    pub url: String,
    /// Normalized URL (dedupe key).
    pub canonical_url: String,
    /// User title, fetched title, or the host as a placeholder.
    pub title: Option<String>,
    /// Fetched description.
    pub description: Option<String>,
    /// Fetched site name.
    pub site_name: Option<String>,
    /// Video or article.
    pub kind: ItemKind,
    /// Local favicon file.
    pub favicon_path: Option<String>,
    /// Local preview image file.
    pub image_path: Option<String>,
    /// User notes (Markdown).
    pub notes_md: String,
    /// Readable-snapshot job state.
    pub snapshot_status: JobStatus,
    /// Why the snapshot failed.
    pub snapshot_error: Option<String>,
    /// When the snapshot was stored.
    pub snapshot_fetched_at: Option<String>,
    /// When the user marked it done; `None` = open.
    pub done_at: Option<String>,
    /// Manual order (unused by the UI so far).
    pub position: i64,
    /// RFC 3339 UTC.
    pub created_at: String,
    /// RFC 3339 UTC.
    pub updated_at: String,
}

/// A checklist entry on a bucket or item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    /// ULID.
    pub id: String,
    /// Owner kind.
    pub parent_type: ParentType,
    /// Owner id.
    pub parent_id: String,
    /// Task text.
    pub title: String,
    /// When it was ticked; `None` = open.
    pub done_at: Option<String>,
    /// Order within the parent, 1-based.
    pub position: i64,
    /// RFC 3339 UTC.
    pub created_at: String,
}

/// A notification scheduled for a bucket, item, or task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    /// ULID.
    pub id: String,
    /// Target kind.
    pub target_type: TargetType,
    /// Target id.
    pub target_id: String,
    /// Originally scheduled time (canonical UTC).
    pub fire_at: String,
    /// Optional note shown in the notification.
    pub message: Option<String>,
    /// When it last fired.
    pub fired_at: Option<String>,
    /// Deferred time; overrides `fire_at` when set.
    pub snoozed_until: Option<String>,
    /// RFC 3339 UTC.
    pub created_at: String,
}

/// A file copied into the data dir.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    /// ULID.
    pub id: String,
    /// Owner kind.
    pub parent_type: ParentType,
    /// Owner id.
    pub parent_id: String,
    /// Original file name.
    pub filename: String,
    /// Absolute path of the stored copy.
    pub path: String,
    /// Guessed MIME type.
    pub mime: String,
    /// Size of the copy.
    pub size_bytes: i64,
    /// RFC 3339 UTC.
    pub created_at: String,
}

/// A voice memo and its transcript.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    /// ULID.
    pub id: String,
    /// Owner kind.
    pub parent_type: ParentType,
    /// Owner id.
    pub parent_id: String,
    /// Absolute path of the 16 kHz WAV.
    pub audio_path: String,
    /// Length in milliseconds.
    pub duration_ms: i64,
    /// Timestamped Markdown transcript.
    pub transcript_md: Option<String>,
    /// Transcription job state.
    pub transcript_status: JobStatus,
    /// Why transcription failed.
    pub transcript_error: Option<String>,
    /// RFC 3339 UTC.
    pub created_at: String,
}

/// One AI provider call and its result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiRun {
    /// ULID.
    pub id: String,
    /// Owner kind.
    pub parent_type: ParentType,
    /// Owner id.
    pub parent_id: String,
    /// What was asked.
    pub action: AiAction,
    /// Which provider ran it.
    pub provider: Provider,
    /// Model id used.
    pub model: String,
    /// Hash of the prompt inputs.
    pub input_hash: String,
    /// The reply (Markdown).
    pub output_md: Option<String>,
    /// Job state.
    pub status: JobStatus,
    /// Why it failed.
    pub error: Option<String>,
    /// RFC 3339 UTC.
    pub created_at: String,
}

/// One full-text search result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// `bucket` | `item` | `snapshot` | `recording` | `ai_run`.
    pub doc_type: String,
    /// Id of the row the doc belongs to.
    pub doc_id: String,
    /// Owning item, if any.
    pub item_id: Option<String>,
    /// Owning bucket, if any.
    pub bucket_id: Option<String>,
    /// Doc title.
    pub title: String,
    /// Matching excerpt with `[hits]` bracketed.
    pub snippet: String,
}

/// Result of an idempotent create: the row, and whether this call inserted it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Created<T> {
    /// The new or pre-existing row.
    pub value: T,
    /// `true` when this call inserted the row; `false` when it already existed.
    pub created: bool,
}
