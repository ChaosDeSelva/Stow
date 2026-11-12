//! Key/value settings (never API keys; those live in the keychain).

use rusqlite::{params, OptionalExtension};

use crate::models::Provider;
use crate::{Db, Result, StowError};

/// Every known settings key, with its default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// Global capture shortcut (Tauri accelerator syntax).
    Hotkey,
    /// Close the Chrome tab after capturing it (`true`/`false`).
    CloseTabAfterCapture,
    /// `anthropic` | `openai`.
    AiProvider,
    /// Model override; empty = provider default.
    AiModel,
    /// `tiny.en` | `base.en` | `small.en`.
    WhisperModel,
    /// `light` | `dark` | `system`.
    Theme,
    /// Port of the extension endpoint; `0` = not yet bound.
    ExtensionPort,
    /// Bearer token the extension presents.
    ExtensionToken,
    /// The onboarding checklist was dismissed.
    OnboardingDismissed,
    /// JSON object of onboarding step → done.
    OnboardingSteps,
    /// The "closing hides to the menu bar" hint was shown.
    CloseToTrayExplained,
    /// Closing the main window quits instead of hiding.
    QuitOnClose,
    /// Launch at login.
    OpenAtLogin,
    /// The user agreed to send content to their AI provider.
    AiConsentGiven,
}

impl Key {
    /// Every key, in declaration order.
    pub const ALL: [Key; 14] = [
        Key::Hotkey,
        Key::CloseTabAfterCapture,
        Key::AiProvider,
        Key::AiModel,
        Key::WhisperModel,
        Key::Theme,
        Key::ExtensionPort,
        Key::ExtensionToken,
        Key::OnboardingDismissed,
        Key::OnboardingSteps,
        Key::CloseToTrayExplained,
        Key::QuitOnClose,
        Key::OpenAtLogin,
        Key::AiConsentGiven,
    ];

    /// Column value in `settings.key`.
    pub fn as_str(self) -> &'static str {
        match self {
            Key::Hotkey => "hotkey",
            Key::CloseTabAfterCapture => "close_tab_after_capture",
            Key::AiProvider => "ai_provider",
            Key::AiModel => "ai_model",
            Key::WhisperModel => "whisper_model",
            Key::Theme => "theme",
            Key::ExtensionPort => "extension_port",
            Key::ExtensionToken => "extension_token",
            Key::OnboardingDismissed => "onboarding_dismissed",
            Key::OnboardingSteps => "onboarding_steps",
            Key::CloseToTrayExplained => "close_to_tray_explained",
            Key::QuitOnClose => "quit_on_close",
            Key::OpenAtLogin => "open_at_login",
            Key::AiConsentGiven => "ai_consent_given",
        }
    }

    /// Value used when the key has never been set.
    pub fn default(self) -> &'static str {
        match self {
            Key::Hotkey => "Ctrl+Cmd+S",
            Key::CloseTabAfterCapture => "true",
            Key::AiProvider => "anthropic",
            Key::AiModel | Key::ExtensionToken => "",
            Key::WhisperModel => "base.en",
            Key::Theme => "light",
            Key::ExtensionPort => "0",
            Key::OnboardingSteps => "{}",
            Key::OnboardingDismissed
            | Key::CloseToTrayExplained
            | Key::QuitOnClose
            | Key::OpenAtLogin
            | Key::AiConsentGiven => "false",
        }
    }
}

/// Raw value, `None` when unset.
pub fn get(db: &Db, key: &str) -> Result<Option<String>> {
    db.with(|c| {
        Ok(
            c.prepare_cached("SELECT value FROM settings WHERE key = ?1")?
                .query_row([key], |r| r.get(0))
                .optional()?,
        )
    })
}

/// Raw value or `default`.
pub fn get_or(db: &Db, key: &str, default: &str) -> Result<String> {
    Ok(get(db, key)?.unwrap_or_else(|| default.to_string()))
}

/// Upsert a raw value.
pub fn set(db: &Db, key: &str, value: &str) -> Result<()> {
    db.with(|c| {
        c.prepare_cached(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )?
        .execute(params![key, value])?;
        Ok(())
    })
}

/// Value of a known key, or its [`Key::default`].
pub fn get_key(db: &Db, key: Key) -> Result<String> {
    get_or(db, key.as_str(), key.default())
}

/// Set a known key.
pub fn set_key(db: &Db, key: Key, value: &str) -> Result<()> {
    set(db, key.as_str(), value)
}

/// A boolean key: stored as `true`/`false`; anything else reads as `false`.
pub fn get_bool(db: &Db, key: Key) -> Result<bool> {
    Ok(get_key(db, key)? == "true")
}

/// Store a boolean key as `true`/`false`.
pub fn set_bool(db: &Db, key: Key, value: bool) -> Result<()> {
    set_key(db, key, if value { "true" } else { "false" })
}

/// `anthropic` | `openai` (trimmed, case-insensitive).
pub fn validate_ai_provider(value: &str) -> Result<Provider> {
    value.trim().to_ascii_lowercase().parse()
}

/// Whisper models Stow offers.
pub const WHISPER_MODELS: [&str; 3] = ["tiny.en", "base.en", "small.en"];

/// One of [`WHISPER_MODELS`]; returns the trimmed value.
pub fn validate_whisper_model(value: &str) -> Result<&str> {
    let v = value.trim();
    if WHISPER_MODELS.contains(&v) {
        Ok(v)
    } else {
        Err(StowError::invalid("whisper_model", value))
    }
}

/// `light` | `dark` | `system` (trimmed, case-insensitive); returns the normalized value.
pub fn validate_theme(value: &str) -> Result<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        "light" => Ok("light"),
        "dark" => Ok("dark"),
        "system" => Ok("system"),
        _ => Err(StowError::invalid("theme", value)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::test_db;

    #[test]
    fn settings_roundtrip() {
        let (_d, db) = test_db();
        assert_eq!(get(&db, "hotkey").unwrap(), None);
        set(&db, "hotkey", "CmdOrCtrl+Shift+S").unwrap();
        set(&db, "hotkey", "Alt+S").unwrap();
        assert_eq!(get(&db, "hotkey").unwrap().as_deref(), Some("Alt+S"));
        assert_eq!(get_or(&db, "missing", "dflt").unwrap(), "dflt");
    }

    #[test]
    fn keys_have_unique_names_and_defaults() {
        let (_d, db) = test_db();
        let mut names: Vec<_> = Key::ALL.iter().map(|k| k.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Key::ALL.len());
        assert_eq!(get_key(&db, Key::Theme).unwrap(), "light");
        assert_eq!(get_key(&db, Key::WhisperModel).unwrap(), "base.en");
        assert_eq!(get_key(&db, Key::Hotkey).unwrap(), "Ctrl+Cmd+S");
        assert_eq!(get_key(&db, Key::OnboardingSteps).unwrap(), "{}");
        set_key(&db, Key::AiModel, "m").unwrap();
        assert_eq!(get(&db, "ai_model").unwrap().as_deref(), Some("m"));
    }

    #[test]
    fn bool_settings() {
        let (_d, db) = test_db();
        assert!(
            get_bool(&db, Key::CloseTabAfterCapture).unwrap(),
            "defaults to true"
        );
        assert!(!get_bool(&db, Key::QuitOnClose).unwrap());
        set_bool(&db, Key::QuitOnClose, true).unwrap();
        assert!(get_bool(&db, Key::QuitOnClose).unwrap());
        assert_eq!(get(&db, "quit_on_close").unwrap().as_deref(), Some("true"));
        set_bool(&db, Key::CloseTabAfterCapture, false).unwrap();
        assert!(!get_bool(&db, Key::CloseTabAfterCapture).unwrap());
    }

    #[test]
    fn validation_helpers() {
        assert_eq!(validate_ai_provider(" OpenAI ").unwrap(), Provider::OpenAi);
        assert_eq!(
            validate_ai_provider("anthropic").unwrap(),
            Provider::Anthropic
        );
        assert_eq!(
            validate_ai_provider("bard").unwrap_err().code(),
            "invalid_input"
        );
        assert_eq!(validate_whisper_model("small.en").unwrap(), "small.en");
        assert!(matches!(
            validate_whisper_model("large-v3"),
            Err(StowError::InvalidInput {
                field: "whisper_model",
                ..
            })
        ));
        assert!(validate_whisper_model("../../x").is_err());
        assert_eq!(validate_theme("Dark").unwrap(), "dark");
        assert!(matches!(
            validate_theme("neon"),
            Err(StowError::InvalidInput { field: "theme", .. })
        ));
    }
}
