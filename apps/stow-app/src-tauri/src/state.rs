//! Shared state handed to Tauri commands via `app.manage(...)`.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Instant;

use stow_core::audio::RecorderHandle;
use stow_core::jobs::JobQueue;
use tauri::tray::TrayIcon;

use crate::events::{HotkeyStatus, Navigate};
use crate::pairing::PairingState;
use crate::tray::TrayModel;

pub struct AppState {
    pub db: Arc<stow_core::Db>,
    pub jobs: JobQueue,
    pub recorder: Arc<RecorderHandle>,
    /// Which bucket/item the in-progress recording belongs to, captured at start.
    pub recording_parent: Mutex<Option<(stow_core::models::ParentType, String)>>,
    /// Result of the last global-shortcut registration.
    pub hotkey: Mutex<HotkeyStatus>,
    pub pairing: Arc<PairingState>,
    /// The menu bar icon, kept so its menu can be rebuilt when the hotkey changes.
    pub tray: Mutex<Option<TrayIcon>>,
    /// What the tray menu currently shows (hotkey, due reminders, recording).
    pub tray_model: Mutex<TrayModel>,
    /// Set once the frontend has been told to flush (or has flushed); the next
    /// `ExitRequested` goes through.
    pub quit_ready: AtomicBool,
    /// Was Stow already the active app when the capture panel opened? If not, hiding the
    /// panel also hides the app so Chrome gets focus back.
    pub stow_was_active: AtomicBool,
    /// Mirror of the `quit_on_close` setting, read on every window close.
    pub quit_on_close: AtomicBool,
    /// The last reminder notification, so reopening the app soon after it fired can
    /// jump to its target (notification clicks are not delivered to us on macOS).
    pub last_fired: Mutex<Option<(Instant, Navigate)>>,
}

/// Lock ignoring poison: every guarded value stays valid if a holder panicked.
pub fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
