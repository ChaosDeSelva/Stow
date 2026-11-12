//! `get_settings` / `update_settings`. Stored values live in the core settings table
//! (defaults from `settings::Key`); the hotkey status and launch-at-login are live state.

use std::collections::BTreeMap;
use std::sync::atomic::Ordering;

use serde::{Deserialize, Serialize};
use stow_core::repo::settings::{self, Key};
use stow_core::Db;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;

use crate::error::{blocking, CmdError, CmdResult};
use crate::server::EXTENSION_ID_KEY;

/// Settings key read at startup to decide whether fetch jobs may touch local addresses.
pub const ALLOW_LOCAL_URLS_KEY: &str = "allow_local_urls";
use crate::state::{lock, AppState};

/// Onboarding checklist entries the frontend knows (others are rejected).
const ONBOARDING_STEPS: [&str; 6] = [
    "captured",
    "automation",
    "notifications",
    "extension",
    "ai_key",
    "whisper",
];
const MAX_AI_MODEL: usize = 200;

#[derive(Debug, Serialize)]
#[allow(clippy::struct_excessive_bools)] // mirrors the contract's Settings type
pub struct Settings {
    /// "light" (default), "dark", or "system".
    pub theme: String,
    pub close_tab_after_capture: bool,
    pub hotkey: String,
    pub ai_provider: String,
    pub ai_model: String,
    pub whisper_model: String,
    pub data_dir: String,
    pub extension_port: u16,
    pub extension_token: String,
    /// Pinned extension id for CORS; empty = any extension (Tauri-phase addition).
    pub extension_id: String,
    pub hotkey_registered: bool,
    pub hotkey_error: Option<String>,
    pub onboarding_dismissed: bool,
    pub onboarding_steps: BTreeMap<String, bool>,
    pub close_to_tray_explained: bool,
    pub quit_on_close: bool,
    pub open_at_login: bool,
    pub ai_consent_given: bool,
    /// Fetch pages/assets on private or loopback addresses (restart required).
    pub allow_local_urls: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct SettingsPatch {
    pub theme: Option<String>,
    pub close_tab_after_capture: Option<bool>,
    pub hotkey: Option<String>,
    pub ai_provider: Option<String>,
    pub ai_model: Option<String>,
    pub whisper_model: Option<String>,
    pub extension_id: Option<String>,
    pub onboarding_dismissed: Option<bool>,
    /// Merged into the stored object; values must be booleans.
    pub onboarding_steps: Option<serde_json::Value>,
    pub close_to_tray_explained: Option<bool>,
    pub quit_on_close: Option<bool>,
    pub open_at_login: Option<bool>,
    pub ai_consent_given: Option<bool>,
    pub allow_local_urls: Option<bool>,
}

/// A patch that passed validation: plain writes plus the parts with side effects.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Validated {
    pub writes: Vec<(&'static str, String)>,
    pub onboarding_steps: Option<BTreeMap<String, bool>>,
    pub hotkey: Option<String>,
    pub open_at_login: Option<bool>,
    pub quit_on_close: Option<bool>,
}

fn bool_str(v: bool) -> String {
    if v { "true" } else { "false" }.to_string()
}

/// Check every field before anything is written; the first bad one is `invalid_input`.
pub fn validate(patch: SettingsPatch) -> CmdResult<Validated> {
    let mut v = Validated::default();
    if let Some(t) = patch.theme {
        v.writes.push((
            Key::Theme.as_str(),
            settings::validate_theme(&t)?.to_string(),
        ));
    }
    if let Some(b) = patch.close_tab_after_capture {
        v.writes
            .push((Key::CloseTabAfterCapture.as_str(), bool_str(b)));
    }
    if let Some(h) = patch.hotkey {
        crate::shortcut::parse(&h)?;
        v.hotkey = Some(h.trim().to_string());
    }
    if let Some(p) = patch.ai_provider {
        v.writes.push((
            Key::AiProvider.as_str(),
            settings::validate_ai_provider(&p)?.as_str().to_string(),
        ));
    }
    if let Some(m) = patch.ai_model {
        let m = m.trim();
        if m.len() > MAX_AI_MODEL || m.chars().any(char::is_control) {
            return Err(CmdError::invalid("ai_model", m));
        }
        v.writes.push((Key::AiModel.as_str(), m.to_string()));
    }
    if let Some(w) = patch.whisper_model {
        v.writes.push((
            Key::WhisperModel.as_str(),
            settings::validate_whisper_model(&w)?.to_string(),
        ));
    }
    if let Some(id) = patch.extension_id {
        let id = id.trim();
        // Chrome extension ids are 32 letters a–p.
        if !(id.is_empty() || (id.len() == 32 && id.bytes().all(|b| (b'a'..=b'p').contains(&b)))) {
            return Err(CmdError::invalid("extension_id", id));
        }
        v.writes.push((EXTENSION_ID_KEY, id.to_string()));
    }
    if let Some(b) = patch.onboarding_dismissed {
        v.writes
            .push((Key::OnboardingDismissed.as_str(), bool_str(b)));
    }
    if let Some(steps) = patch.onboarding_steps {
        v.onboarding_steps = Some(validate_steps(&steps)?);
    }
    if let Some(b) = patch.close_to_tray_explained {
        v.writes
            .push((Key::CloseToTrayExplained.as_str(), bool_str(b)));
    }
    if let Some(b) = patch.quit_on_close {
        v.writes.push((Key::QuitOnClose.as_str(), bool_str(b)));
        v.quit_on_close = Some(b);
    }
    if let Some(b) = patch.ai_consent_given {
        v.writes.push((Key::AiConsentGiven.as_str(), bool_str(b)));
    }
    if let Some(b) = patch.allow_local_urls {
        v.writes.push((ALLOW_LOCAL_URLS_KEY, bool_str(b)));
    }
    v.open_at_login = patch.open_at_login;
    Ok(v)
}

fn validate_steps(steps: &serde_json::Value) -> CmdResult<BTreeMap<String, bool>> {
    let bad = || CmdError::invalid("onboarding_steps", steps.to_string());
    let obj = steps.as_object().ok_or_else(bad)?;
    obj.iter()
        .map(|(k, val)| match val.as_bool() {
            Some(b) if ONBOARDING_STEPS.contains(&k.as_str()) => Ok((k.clone(), b)),
            _ => Err(bad()),
        })
        .collect()
}

fn stored_steps(db: &Db) -> stow_core::Result<BTreeMap<String, bool>> {
    Ok(serde_json::from_str(&settings::get_key(db, Key::OnboardingSteps)?).unwrap_or_default())
}

/// Apply validated writes (the hotkey only when given, i.e. after it registered).
pub fn write(db: &Db, v: &Validated, hotkey: Option<&str>) -> stow_core::Result<()> {
    for (k, val) in &v.writes {
        settings::set(db, k, val)?;
    }
    if let Some(patch) = &v.onboarding_steps {
        let mut steps = stored_steps(db)?;
        steps.extend(patch.iter().map(|(k, b)| (k.clone(), *b)));
        settings::set_key(db, Key::OnboardingSteps, &serde_json::to_string(&steps)?)?;
    }
    if let Some(h) = hotkey {
        settings::set_key(db, Key::Hotkey, h)?;
    }
    Ok(())
}

/// Stored settings; `hotkey_*` and `open_at_login` are filled in by the command.
pub fn load(db: &Db) -> stow_core::Result<Settings> {
    Ok(Settings {
        theme: settings::get_key(db, Key::Theme)?,
        close_tab_after_capture: settings::get_bool(db, Key::CloseTabAfterCapture)?,
        hotkey: settings::get_key(db, Key::Hotkey)?,
        ai_provider: settings::get_key(db, Key::AiProvider)?,
        ai_model: settings::get_key(db, Key::AiModel)?,
        whisper_model: settings::get_key(db, Key::WhisperModel)?,
        data_dir: db.data_dir().to_string_lossy().into_owned(),
        extension_port: settings::get_key(db, Key::ExtensionPort)?
            .parse()
            .unwrap_or(0),
        extension_token: settings::get_key(db, Key::ExtensionToken)?,
        extension_id: settings::get_or(db, EXTENSION_ID_KEY, "")?,
        hotkey_registered: false,
        hotkey_error: None,
        onboarding_dismissed: settings::get_bool(db, Key::OnboardingDismissed)?,
        onboarding_steps: stored_steps(db)?,
        close_to_tray_explained: settings::get_bool(db, Key::CloseToTrayExplained)?,
        quit_on_close: settings::get_bool(db, Key::QuitOnClose)?,
        open_at_login: settings::get_bool(db, Key::OpenAtLogin)?,
        ai_consent_given: settings::get_bool(db, Key::AiConsentGiven)?,
        allow_local_urls: settings::get(db, ALLOW_LOCAL_URLS_KEY)?.is_some_and(|v| v == "true"),
    })
}

async fn current(app: &AppHandle, state: &AppState) -> CmdResult<Settings> {
    let mut s = super::db(state, load).await?;
    {
        let hk = lock(&state.hotkey);
        s.hotkey_registered = hk.registered && hk.hotkey == s.hotkey;
        s.hotkey_error.clone_from(&hk.error);
    }
    let app = app.clone();
    let stored = s.open_at_login;
    s.open_at_login =
        blocking(move || Ok::<_, CmdError>(app.autolaunch().is_enabled().unwrap_or(stored)))
            .await?;
    Ok(s)
}

#[tauri::command]
pub async fn get_settings(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Settings> {
    current(&app, &state).await
}

/// The subset of settings the capture panel needs; it never sees the extension token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaptureSettings {
    pub close_tab_after_capture: bool,
    pub hotkey: String,
    pub theme: String,
}

pub fn load_capture(db: &Db) -> stow_core::Result<CaptureSettings> {
    Ok(CaptureSettings {
        close_tab_after_capture: settings::get_bool(db, Key::CloseTabAfterCapture)?,
        hotkey: settings::get_key(db, Key::Hotkey)?,
        theme: settings::get_key(db, Key::Theme)?,
    })
}

#[tauri::command]
pub async fn capture_settings(state: State<'_, AppState>) -> CmdResult<CaptureSettings> {
    super::db(&state, load_capture).await
}

/// The one setting the capture panel changes (its "close the tab" checkbox).
#[tauri::command]
pub async fn set_close_tab_after_capture(
    state: State<'_, AppState>,
    value: bool,
) -> CmdResult<CaptureSettings> {
    super::db(&state, move |d| {
        settings::set_key(d, Key::CloseTabAfterCapture, &bool_str(value))?;
        load_capture(d)
    })
    .await
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> CmdResult<Settings> {
    let v = validate(patch)?;
    let before = super::db(&state, |d| settings::get_key(d, Key::Hotkey)).await?;
    let new_hotkey = v.hotkey.clone().filter(|h| *h != before);
    let status = match &new_hotkey {
        Some(h) => Some(crate::shortcut::replace(&app, &before, h)?),
        None => None,
    };
    if let Some(enable) = v.open_at_login {
        let a = app.clone();
        blocking(move || {
            let m = a.autolaunch();
            if enable { m.enable() } else { m.disable() }
                .map_err(|e| CmdError::internal(format!("launch at login: {e}")))
        })
        .await?;
    }
    let mut v = v;
    if let Some(b) = v.open_at_login {
        v.writes.push((Key::OpenAtLogin.as_str(), bool_str(b)));
    }
    let hk = new_hotkey.clone();
    let quit_on_close = v.quit_on_close;
    super::db(&state, move |d| write(d, &v, hk.as_deref())).await?;
    if let Some(b) = quit_on_close {
        state.quit_on_close.store(b, Ordering::SeqCst);
    }
    if let (Some(h), Some(status)) = (new_hotkey, status) {
        crate::shortcut::publish(&app, status);
        crate::tray::rebuild_menu(&app, &h);
    }
    current(&app, &state).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn patch(v: serde_json::Value) -> SettingsPatch {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn settings_validation_rejects_bad_values_with_invalid_input() {
        for bad in [
            json!({"theme": "neon"}),
            json!({"ai_provider": "gemini"}),
            json!({"whisper_model": "large"}),
            json!({"hotkey": "Banana+"}),
            json!({"hotkey": "S"}),
            json!({"ai_model": "x".repeat(201)}),
            json!({"extension_id": "not-an-id"}),
            json!({"onboarding_steps": {"captured": "yes"}}),
            json!({"onboarding_steps": {"unknown_step": true}}),
            json!({"onboarding_steps": [true]}),
        ] {
            let e = validate(patch(bad.clone())).unwrap_err();
            assert_eq!(e.code, "invalid_input", "{bad}");
        }
    }

    #[test]
    fn settings_validation_normalizes() {
        let v = validate(patch(json!({
            "theme": " Dark ",
            "ai_provider": "OpenAI",
            "whisper_model": "small.en",
            "hotkey": " Ctrl+Cmd+K ",
            "quit_on_close": true,
            "open_at_login": false,
            "onboarding_steps": {"captured": true},
            "extension_id": "abcdefghijklmnopabcdefghijklmnop",
        })))
        .unwrap();
        assert!(v.writes.contains(&("theme", "dark".into())));
        assert!(v.writes.contains(&("ai_provider", "openai".into())));
        assert!(v.writes.contains(&("whisper_model", "small.en".into())));
        assert!(v.writes.contains(&("quit_on_close", "true".into())));
        assert_eq!(v.hotkey.as_deref(), Some("Ctrl+Cmd+K"));
        assert_eq!(v.open_at_login, Some(false));
        assert_eq!(v.quit_on_close, Some(true));
        assert!(
            !v.writes.iter().any(|(k, _)| *k == "hotkey"),
            "hotkey is written only after it registers"
        );
    }

    #[test]
    fn settings_invalid_patch_writes_nothing() {
        let dir = crate::testutil::TempDir::new();
        let db = Db::open(dir.path()).unwrap();
        // theme is fine, whisper_model is not: validate fails before any write.
        assert!(validate(patch(json!({"theme": "dark", "whisper_model": "huge"}))).is_err());
        assert_eq!(load(&db).unwrap().theme, "light");
    }

    #[test]
    fn capture_settings_expose_only_the_panel_fields() {
        let dir = crate::testutil::TempDir::new();
        let db = Db::open(dir.path()).unwrap();
        let c = load_capture(&db).unwrap();
        assert_eq!(
            c,
            CaptureSettings {
                close_tab_after_capture: true,
                hotkey: "Ctrl+Cmd+S".into(),
                theme: "light".into()
            }
        );
        settings::set_key(&db, Key::CloseTabAfterCapture, &bool_str(false)).unwrap();
        assert!(!load_capture(&db).unwrap().close_tab_after_capture);
        let v = serde_json::to_value(load_capture(&db).unwrap()).unwrap();
        assert_eq!(v.as_object().unwrap().len(), 3, "no token leaks: {v}");
    }

    #[test]
    fn settings_load_defaults_and_roundtrip() {
        let dir = crate::testutil::TempDir::new();
        let db = Db::open(dir.path()).unwrap();
        let s = load(&db).unwrap();
        assert_eq!(s.hotkey, "Ctrl+Cmd+S");
        assert_eq!(s.theme, "light");
        assert!(s.close_tab_after_capture);
        assert!(!s.onboarding_dismissed && !s.quit_on_close && !s.ai_consent_given);
        assert!(s.onboarding_steps.is_empty());
        assert_eq!(s.extension_id, "");

        let v = validate(patch(json!({
            "onboarding_steps": {"captured": true},
            "ai_consent_given": true,
            "hotkey": "Alt+S",
        })))
        .unwrap();
        write(&db, &v, None).unwrap();
        let v2 = validate(patch(json!({"onboarding_steps": {"whisper": false}}))).unwrap();
        write(&db, &v2, Some("Alt+S")).unwrap();
        let s = load(&db).unwrap();
        assert!(s.ai_consent_given);
        assert_eq!(s.hotkey, "Alt+S");
        assert_eq!(
            s.onboarding_steps,
            BTreeMap::from([("captured".into(), true), ("whisper".into(), false)]),
            "steps merge"
        );
        let v = serde_json::to_value(&s).unwrap();
        for k in [
            "hotkey_registered",
            "hotkey_error",
            "onboarding_dismissed",
            "onboarding_steps",
            "close_to_tray_explained",
            "quit_on_close",
            "open_at_login",
            "ai_consent_given",
        ] {
            assert!(v.get(k).is_some(), "missing {k}: {v}");
        }
    }
}
