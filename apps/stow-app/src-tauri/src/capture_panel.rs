//! macOS window behavior for the quick-capture panel: it must appear on whatever Space
//! is active, including a full-screen Chrome Space, and float above it.
//!
//! The panel stays an ordinary `NSWindow`, not an `NSPanel`: `tauri-nspanel` would make it
//! non-activating, but it hard-requires tauri's `macos-private-api` feature and swaps the
//! window's class at runtime. Focus is handed back to Chrome by `hide_capture` instead
//! (`app.hide()` when Stow was not the active app).
//!
//! This module and `permissions.rs` are the only ones allowed to use `unsafe`: the
//! collection behavior has no Tauri API, so it goes through `objc2-app-kit`.
#![allow(unsafe_code)]

#[cfg(target_os = "macos")]
use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
#[cfg(target_os = "macos")]
use tauri::WebviewWindow;

/// `current` with the Space behaviors the panel needs. `CanJoinAllSpaces` puts it on the
/// active Space without switching Spaces, and `FullScreenAuxiliary` lets it show over a
/// full-screen app. `MoveToActiveSpace` and `Managed` are mutually exclusive with
/// `CanJoinAllSpaces` (Apple's docs), so they are cleared rather than combined.
pub fn panel_collection_behavior(current: usize) -> usize {
    const CAN_JOIN_ALL_SPACES: usize = 1 << 0;
    const MOVE_TO_ACTIVE_SPACE: usize = 1 << 1;
    const MANAGED: usize = 1 << 2;
    const FULL_SCREEN_AUXILIARY: usize = 1 << 8;
    (current & !(MOVE_TO_ACTIVE_SPACE | MANAGED)) | CAN_JOIN_ALL_SPACES | FULL_SCREEN_AUXILIARY
}

/// Apply the Space behaviors to the capture window. Must run on the main thread (setup
/// does). The floating level comes from `alwaysOnTop` in tauri.conf.json.
#[cfg(target_os = "macos")]
pub fn configure(window: &WebviewWindow) -> tauri::Result<()> {
    window.set_visible_on_all_workspaces(true)?;
    let ptr = window.ns_window()?;
    // SAFETY: `ns_window` returns the live NSWindow tao owns for this window, valid while
    // `window` is; we are on the main thread, which `NSWindow` (MainThreadOnly) requires.
    let ns_window = unsafe { &*ptr.cast::<NSWindow>() };
    let behavior = panel_collection_behavior(ns_window.collectionBehavior().0);
    ns_window.setCollectionBehavior(NSWindowCollectionBehavior(behavior));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_collection_behavior_joins_all_spaces_and_fullscreen() {
        let b = panel_collection_behavior(0);
        assert_eq!(b, (1 << 0) | (1 << 8));
        // Existing unrelated bits survive; the mutually exclusive Space bits do not.
        let b = panel_collection_behavior((1 << 1) | (1 << 2) | (1 << 6));
        assert_eq!(b, (1 << 0) | (1 << 6) | (1 << 8));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn panel_collection_behavior_matches_appkit_constants() {
        let b = NSWindowCollectionBehavior(panel_collection_behavior(0));
        assert!(b.contains(NSWindowCollectionBehavior::CanJoinAllSpaces));
        assert!(b.contains(NSWindowCollectionBehavior::FullScreenAuxiliary));
        assert!(!b.contains(NSWindowCollectionBehavior::MoveToActiveSpace));
    }
}
