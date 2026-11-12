use std::sync::atomic::Ordering;

use stow_core::models::TargetType;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::error::{require_id, CmdResult};
use crate::events::{self, Navigate};
use crate::state::AppState;

#[tauri::command]
pub fn hide_capture(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(w) = app.get_webview_window("capture") {
        w.hide()?;
    }
    #[cfg(target_os = "macos")]
    if !state.stow_was_active.load(Ordering::SeqCst) {
        // Deactivates Stow so the previously active app (Chrome) comes back to the front.
        let _ = app.hide();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = state;
    Ok(())
}

pub fn show_main_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("main") {
        w.show()?;
        w.unminimize().ok();
        w.set_focus()?;
    }
    Ok(())
}

/// Show main and select `target` there.
pub fn navigate(app: &AppHandle, target: Navigate) -> tauri::Result<()> {
    show_main_window(app)?;
    app.emit_to("main", events::NAVIGATE, target)
}

#[tauri::command]
pub fn show_main(
    app: AppHandle,
    target_type: Option<TargetType>,
    target_id: Option<String>,
) -> CmdResult<()> {
    match (target_type, target_id) {
        (Some(target_type), Some(target_id)) => {
            require_id("target_id", &target_id)?;
            navigate(
                &app,
                Navigate {
                    target_type,
                    target_id,
                },
            )?;
        }
        _ => show_main_window(&app)?,
    }
    Ok(())
}

/// The frontend has flushed its autosaves after `app-will-quit`: exit now instead of
/// waiting for the 800 ms fallback.
#[tauri::command]
#[allow(clippy::unnecessary_wraps)] // every command returns CmdResult for a uniform IPC shape
pub fn ready_to_quit(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    state.quit_ready.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

/// Show the quick-capture panel and tell it to grab the front Chrome tab.
pub fn open_capture(app: &AppHandle) {
    let main_focused = app
        .get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false);
    if let Some(state) = app.try_state::<AppState>() {
        state.stow_was_active.store(main_focused, Ordering::SeqCst);
    }
    if let Some(w) = app.get_webview_window("capture") {
        let _ = w.center();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = app.emit_to("capture", events::CAPTURE_OPEN, events::Empty {});
    }
}
