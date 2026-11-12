//! Command error type. Every command rejects with `{code, message}`; the codes are the
//! list in docs/superpowers/plans/tauri-contract.md ("Errors become structured").

use serde::Serialize;
use stow_core::StowError;

use crate::chrome::ChromeError;

/// What a rejected `invoke` carries to the frontend.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CmdError {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Human-readable detail (shown behind a "Details" disclosure).
    pub message: String,
}

pub type CmdResult<T> = Result<T, CmdError>;

impl CmdError {
    pub fn new(code: &'static str, message: impl Into<String>) -> CmdError {
        CmdError {
            code,
            message: message.into(),
        }
    }

    /// `invalid_input` naming the argument and the rejected value.
    /// The same background job is already queued or running.
    pub fn busy(message: &str) -> Self {
        Self {
            code: "busy",
            message: message.to_string(),
        }
    }

    pub fn invalid(field: &'static str, value: impl Into<String>) -> CmdError {
        StowError::invalid(field, value).into()
    }

    pub fn internal(message: impl Into<String>) -> CmdError {
        CmdError::new("internal", message)
    }
}

/// Reject anything but a ULID before it reaches the database.
pub fn require_id(field: &'static str, id: &str) -> CmdResult<()> {
    Ok(stow_core::ids::require_ulid(field, id)?)
}

impl From<StowError> for CmdError {
    fn from(e: StowError) -> CmdError {
        CmdError::new(e.code(), e.to_string())
    }
}

impl From<ChromeError> for CmdError {
    fn from(e: ChromeError) -> CmdError {
        let code = match &e {
            ChromeError::NotRunning => "chrome_not_running",
            ChromeError::PermissionDenied => "permission_denied",
            ChromeError::NoTab => "no_tab",
            ChromeError::Script(_) => "script_error",
        };
        CmdError::new(code, e.to_string())
    }
}

impl From<tauri::Error> for CmdError {
    fn from(e: tauri::Error) -> CmdError {
        CmdError::internal(e.to_string())
    }
}

impl From<String> for CmdError {
    fn from(message: String) -> CmdError {
        CmdError::internal(message)
    }
}

impl From<tokio::task::JoinError> for CmdError {
    fn from(e: tokio::task::JoinError) -> CmdError {
        CmdError::internal(format!("background task failed: {e}"))
    }
}

/// Run blocking work (files, Keychain, `AppleScript`, the recorder) off the main thread.
pub async fn blocking<T, E>(f: impl FnOnce() -> Result<T, E> + Send + 'static) -> CmdResult<T>
where
    T: Send + 'static,
    E: Into<CmdError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use stow_core::EntityKind;

    #[test]
    fn cmd_error_codes_match_contract() {
        let cases: Vec<(CmdError, &str)> = vec![
            (
                StowError::not_found(EntityKind::Item, "x").into(),
                "not_found",
            ),
            (
                StowError::InvalidUrl {
                    input: "chrome://x".into(),
                    reason: "scheme".into(),
                }
                .into(),
                "invalid_url",
            ),
            (CmdError::invalid("id", "nope"), "invalid_input"),
            (StowError::NotHtml.into(), "not_html"),
            (StowError::TooLarge { limit_bytes: 1 }.into(), "too_large"),
            (StowError::Keychain("k".into()).into(), "keychain"),
            (StowError::Audio("a".into()).into(), "audio"),
            (StowError::Transcribe("t".into()).into(), "transcribe"),
            (
                StowError::Provider {
                    provider: "anthropic",
                    status: Some(401),
                    message: "bad key".into(),
                }
                .into(),
                "provider",
            ),
            (StowError::Hotkey("taken".into()).into(), "hotkey"),
            (StowError::Internal("bug".into()).into(), "internal"),
            (ChromeError::NotRunning.into(), "chrome_not_running"),
            (ChromeError::PermissionDenied.into(), "permission_denied"),
            (ChromeError::NoTab.into(), "no_tab"),
            (ChromeError::Script("boom".into()).into(), "script_error"),
            (String::from("weird").into(), "internal"),
        ];
        for (err, code) in cases {
            assert_eq!(err.code, code, "{err:?}");
            assert!(!err.message.is_empty(), "{err:?}");
        }
    }

    #[test]
    fn cmd_error_serializes_as_code_and_message() {
        let e = CmdError::invalid("id", "nope");
        let v = serde_json::to_value(&e).unwrap();
        assert_eq!(v["code"], "invalid_input");
        assert!(v["message"].as_str().unwrap().contains("nope"), "{v}");
        assert_eq!(v.as_object().unwrap().len(), 2);
        let p: CmdError = StowError::Provider {
            provider: "openai",
            status: Some(429),
            message: "slow down".into(),
        }
        .into();
        assert!(p.message.starts_with("openai 429"), "{}", p.message);
        let s: CmdError = ChromeError::Script("boom".into()).into();
        assert_eq!(s.message, "script_error: boom");
    }

    #[test]
    fn require_id_accepts_ulids_only() {
        assert!(require_id("id", &stow_core::ids::new_id()).is_ok());
        for bad in [
            "",
            "abc",
            "../../etc/passwd",
            "01DOESNOTEXIST00000000000I",
            "8ZZZZZZZZZZZZZZZZZZZZZZZZZ",
        ] {
            assert_eq!(
                require_id("id", bad).unwrap_err().code,
                "invalid_input",
                "{bad}"
            );
        }
    }
}
