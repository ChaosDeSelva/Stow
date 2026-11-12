//! The one error type of stow-core. Every variant maps to a stable machine-readable
//! [`StowError::code`] that the Tauri layer forwards to the frontend unchanged.

use std::fmt;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// `std::result::Result` with [`StowError`].
pub type Result<T> = std::result::Result<T, StowError>;

/// Which kind of row a [`StowError::NotFound`] refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    /// A bucket.
    Bucket,
    /// A saved link.
    Item,
    /// A task on a bucket or item.
    Task,
    /// A reminder.
    Reminder,
    /// An attached file.
    Attachment,
    /// A voice recording.
    Recording,
    /// An AI run.
    AiRun,
}

impl EntityKind {
    /// Lowercase name, as used in messages and search doc types.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bucket => "bucket",
            Self::Item => "item",
            Self::Task => "task",
            Self::Reminder => "reminder",
            Self::Attachment => "attachment",
            Self::Recording => "recording",
            Self::AiRun => "ai_run",
        }
    }
}

impl fmt::Display for EntityKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything that can go wrong in stow-core.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum StowError {
    /// SQLite failed.
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    /// A filesystem operation failed; `op` is a short verb such as `copy` or `create dir`.
    #[error("could not {op} {}: {source}", path.display())]
    Io {
        /// What was being done.
        op: &'static str,
        /// The file or directory involved.
        path: PathBuf,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// No row with this id.
    #[error("{kind} not found: {id}")]
    NotFound {
        /// The table the id was looked up in.
        kind: EntityKind,
        /// The id (or URL) that was not found.
        id: String,
    },
    /// The input is not an http(s) URL Stow can save.
    #[error("invalid url {input:?}: {reason}")]
    InvalidUrl {
        /// What the caller passed.
        input: String,
        /// Why it was rejected.
        reason: String,
    },
    /// A non-URL argument failed validation.
    #[error("invalid {field}: {value:?}")]
    InvalidInput {
        /// Argument or setting name.
        field: &'static str,
        /// The rejected value.
        value: String,
    },
    /// An HTTP request failed (connect, timeout, or an error status).
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    /// The response was not an HTML page.
    #[error("not an html page")]
    NotHtml,
    /// A download exceeded its size cap.
    #[error("larger than {} MB", limit_bytes / (1024 * 1024))]
    TooLarge {
        /// The cap that was exceeded, in bytes.
        limit_bytes: u64,
    },
    /// Readability / Markdown extraction failed.
    #[error("extraction failed: {0}")]
    Extract(String),
    /// JSON (de)serialization failed.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// The OS keychain refused or failed.
    #[error("keychain error: {0}")]
    Keychain(String),
    /// Audio capture or WAV handling failed.
    #[error("audio error: {0}")]
    Audio(String),
    /// Whisper model download or transcription failed.
    #[error("transcription error: {0}")]
    Transcribe(String),
    /// An AI provider call failed. The message starts with the provider and status.
    #[error("{}", provider_message(provider, *status, message))]
    Provider {
        /// `anthropic` or `openai`.
        provider: &'static str,
        /// HTTP status, when the provider answered at all.
        status: Option<u16>,
        /// Human-readable detail (an excerpt of the error body, or why no call was made).
        message: String,
    },
    /// Registering the global shortcut failed.
    #[error("hotkey error: {0}")]
    Hotkey(String),
    /// The database was written by a newer Stow than this build.
    #[error(
        "database schema version {found} is newer than this build supports ({supported}); update Stow"
    )]
    SchemaTooNew {
        /// `PRAGMA user_version` found on disk.
        found: i64,
        /// Highest version this build knows.
        supported: i64,
    },
    /// A bug or an unexpected state (a panicked worker, a closed channel, ...).
    #[error("internal error: {0}")]
    Internal(String),
}

fn provider_message(provider: &str, status: Option<u16>, message: &str) -> String {
    match status {
        Some(s) => format!("{provider} {s}: {message}"),
        None => format!("{provider}: {message}"),
    }
}

impl StowError {
    /// Stable code for the frontend; see `docs/superpowers/plans/tauri-contract.md`.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "db",
            Self::Io { .. } => "io",
            Self::NotFound { .. } => "not_found",
            Self::InvalidUrl { .. } => "invalid_url",
            Self::InvalidInput { .. } => "invalid_input",
            Self::Http(_) => "http",
            Self::NotHtml => "not_html",
            Self::TooLarge { .. } => "too_large",
            Self::Keychain(_) => "keychain",
            Self::Audio(_) => "audio",
            Self::Transcribe(_) => "transcribe",
            Self::Provider { .. } => "provider",
            Self::Hotkey(_) => "hotkey",
            Self::SchemaTooNew { .. } => "schema",
            Self::Extract(_) | Self::Json(_) | Self::Internal(_) => "internal",
        }
    }

    /// Shorthand for [`StowError::NotFound`].
    pub fn not_found(kind: EntityKind, id: impl Into<String>) -> Self {
        Self::NotFound {
            kind,
            id: id.into(),
        }
    }

    /// Shorthand for [`StowError::InvalidInput`].
    pub fn invalid(field: &'static str, value: impl Into<String>) -> Self {
        Self::InvalidInput {
            field,
            value: value.into(),
        }
    }

    /// True for [`StowError::NotFound`].
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }
}

/// Attach the operation and path to an `io::Error`: `fs::copy(a, b).io_ctx("copy", b)?`.
pub trait IoContext<T> {
    /// Convert into [`StowError::Io`] carrying `op` and `path`.
    fn io_ctx(self, op: &'static str, path: impl AsRef<Path>) -> Result<T>;
}

impl<T> IoContext<T> for std::result::Result<T, std::io::Error> {
    fn io_ctx(self, op: &'static str, path: impl AsRef<Path>) -> Result<T> {
        self.map_err(|source| StowError::Io {
            op,
            path: path.as_ref().to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_stable() {
        let io = || std::io::Error::other("x");
        let json = serde_json::from_str::<serde_json::Value>("{").unwrap_err();
        let cases: Vec<(StowError, &str)> = vec![
            (StowError::Db(rusqlite::Error::InvalidQuery), "db"),
            (
                StowError::Io {
                    op: "copy",
                    path: "/a".into(),
                    source: io(),
                },
                "io",
            ),
            (StowError::not_found(EntityKind::Item, "x"), "not_found"),
            (
                StowError::InvalidUrl {
                    input: "x".into(),
                    reason: "y".into(),
                },
                "invalid_url",
            ),
            (StowError::invalid("theme", "neon"), "invalid_input"),
            (StowError::NotHtml, "not_html"),
            (StowError::TooLarge { limit_bytes: 5 }, "too_large"),
            (StowError::Extract("x".into()), "internal"),
            (StowError::Json(json), "internal"),
            (StowError::Keychain("x".into()), "keychain"),
            (StowError::Audio("x".into()), "audio"),
            (StowError::Transcribe("x".into()), "transcribe"),
            (
                StowError::Provider {
                    provider: "openai",
                    status: Some(401),
                    message: "m".into(),
                },
                "provider",
            ),
            (StowError::Hotkey("x".into()), "hotkey"),
            (
                StowError::SchemaTooNew {
                    found: 9,
                    supported: 2,
                },
                "schema",
            ),
            (StowError::Internal("x".into()), "internal"),
        ];
        for (err, code) in cases {
            assert_eq!(err.code(), code, "{err:?}");
        }
    }

    #[test]
    fn io_error_carries_path() {
        let missing = Path::new("/definitely/not/here.txt");
        let err = std::fs::read(missing).io_ctx("read", missing).unwrap_err();
        match &err {
            StowError::Io { op, path, .. } => {
                assert_eq!(*op, "read");
                assert_eq!(path, missing);
            }
            other => panic!("expected Io, got {other:?}"),
        }
        let msg = err.to_string();
        assert!(
            msg.contains("read") && msg.contains("/definitely/not/here.txt"),
            "{msg}"
        );
        assert!(std::error::Error::source(&err).is_some(), "source kept");
    }

    #[test]
    fn display_messages_are_human_readable() {
        assert_eq!(
            StowError::not_found(EntityKind::AiRun, "01X").to_string(),
            "ai_run not found: 01X"
        );
        assert_eq!(
            StowError::TooLarge {
                limit_bytes: 5 * 1024 * 1024
            }
            .to_string(),
            "larger than 5 MB"
        );
        let p = StowError::Provider {
            provider: "anthropic",
            status: Some(500),
            message: "boom".into(),
        };
        assert_eq!(p.to_string(), "anthropic 500: boom");
        let p = StowError::Provider {
            provider: "openai",
            status: None,
            message: "no api key".into(),
        };
        assert_eq!(p.to_string(), "openai: no api key");
    }
}
