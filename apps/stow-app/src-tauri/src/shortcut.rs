//! Global hotkey that opens the quick-capture panel. A failed registration (another app
//! owns the combination) is reported through `HotkeyStatus`, never fatal.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::error::{CmdError, CmdResult};
use crate::events::{self, HotkeyStatus};
use crate::state::{lock, AppState};

/// Parse an accelerator such as `Ctrl+Cmd+S`. Unparseable strings and bare keys (no
/// Cmd/Ctrl/Alt/Shift) are `invalid_input`: a global shortcut without a modifier would
/// swallow ordinary typing in every app.
pub fn parse(hotkey: &str) -> CmdResult<Shortcut> {
    let invalid = || CmdError::invalid("hotkey", hotkey);
    let shortcut = hotkey.trim().parse::<Shortcut>().map_err(|_| invalid())?;
    if shortcut.mods.is_empty() {
        return Err(invalid());
    }
    Ok(shortcut)
}

fn try_register(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let shortcut = parse(hotkey).map_err(|e| e.message)?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                crate::commands::window::open_capture(app);
            }
        })
        .map_err(|e| format!("could not register {hotkey}: {e}"))
}

/// Register `hotkey` and report the outcome.
pub fn register(app: &AppHandle, hotkey: &str) -> HotkeyStatus {
    let r = try_register(app, hotkey);
    if let Err(e) = &r {
        tracing::warn!("{e}");
    }
    HotkeyStatus {
        hotkey: hotkey.to_string(),
        registered: r.is_ok(),
        error: r.err(),
    }
}

/// Store the status and tell every window.
pub fn publish(app: &AppHandle, status: HotkeyStatus) {
    if let Some(state) = app.try_state::<AppState>() {
        *lock(&state.hotkey) = status.clone();
    }
    if let Err(e) = app.emit(events::HOTKEY_STATUS, status) {
        tracing::warn!("emit failed: {e}");
    }
}

/// Swap `old` for `new`. On failure the old shortcut is put back so the user is never
/// left without one, and the error is a `hotkey` `CmdError`.
pub fn replace(app: &AppHandle, old: &str, new: &str) -> CmdResult<HotkeyStatus> {
    if let Ok(old_sc) = parse(old) {
        let _ = app.global_shortcut().unregister(old_sc);
    }
    let status = register(app, new);
    if status.registered {
        return Ok(status);
    }
    let restored = register(app, old);
    publish(app, restored);
    Err(CmdError::new(
        "hotkey",
        status
            .error
            .unwrap_or_else(|| format!("could not register {new}")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkey_parse_accepts_default_and_rejects_garbage() {
        use stow_core::repo::settings::Key;
        assert!(parse(Key::Hotkey.default()).is_ok());
        assert!(parse("CmdOrCtrl+Shift+S").is_ok());
        for bad in ["", "Ctrl+", "NotAKey+Q", "Ctrl+Cmd+S+T+banana"] {
            assert_eq!(parse(bad).unwrap_err().code, "invalid_input", "{bad}");
        }
    }

    #[test]
    fn hotkey_parse_requires_a_modifier() {
        for bare in ["S", "F5", "Space", " Enter "] {
            assert_eq!(parse(bare).unwrap_err().code, "invalid_input", "{bare}");
        }
        for ok in ["Shift+F5", "Alt+Space", "Ctrl+S", "Cmd+Enter", "Super+K"] {
            assert!(parse(ok).is_ok(), "{ok}");
        }
    }
}
