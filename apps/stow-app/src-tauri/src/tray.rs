//! Menu bar icon so Stow keeps running (and firing reminders) with the window closed.
//! The menu is rebuilt from a [`TrayModel`] whenever the hotkey, the due reminders or
//! the recording state change.

use std::time::Duration;

use stow_core::repo::reminders;
use stow_core::{CaptureRequest, Db, Stow};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_notification::NotificationExt;

use crate::events::{self, Navigate};
use crate::state::{lock, AppState};

/// How many "Due:" entries the menu shows.
pub const MAX_DUE: usize = 3;
/// Reminders whose time came within this window still count as due.
pub const DUE_LOOKBACK: Duration = Duration::from_secs(24 * 60 * 60);
/// Longest title in a "Due:" label before it is cut with an ellipsis.
const MAX_TITLE_CHARS: usize = 40;

/// Menu item id prefix for due reminders; the rest is the reminder id.
pub const DUE_PREFIX: &str = "due:";
pub const STOP_RECORDING_ID: &str = "stop-recording";

/// A reminder whose time has come, shown as "Due: <title>".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DueReminder {
    pub reminder_id: String,
    pub title: String,
}

/// Everything the menu's dynamic parts depend on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayModel {
    pub hotkey: String,
    pub due: Vec<DueReminder>,
    pub recording: bool,
}

/// One menu entry: `(id, label)`.
pub type Entry = (String, String);

/// The context section (due reminders, then the recording control), in menu order.
/// Empty when there is nothing to show, so the caller can skip the separator.
pub fn context_entries(model: &TrayModel) -> Vec<Entry> {
    let mut out: Vec<Entry> = model
        .due
        .iter()
        .take(MAX_DUE)
        .map(|d| {
            (
                format!("{DUE_PREFIX}{}", d.reminder_id),
                format!("Due: {}", truncate(&d.title)),
            )
        })
        .collect();
    if model.recording {
        out.push((
            STOP_RECORDING_ID.to_string(),
            "Recording… ■ Stop".to_string(),
        ));
    }
    out
}

fn truncate(title: &str) -> String {
    let title = title.trim();
    let title = if title.is_empty() { "Reminder" } else { title };
    match title.char_indices().nth(MAX_TITLE_CHARS) {
        Some((cut, _)) => format!("{}…", title[..cut].trim_end()),
        None => title.to_string(),
    }
}

/// The reminder id of a `due:` menu entry.
pub fn due_reminder_id(menu_id: &str) -> Option<&str> {
    menu_id.strip_prefix(DUE_PREFIX)
}

/// Reminders whose (snoozed) time fell in the last [`DUE_LOOKBACK`], newest first.
pub fn due_reminders(db: &Db, now: &str) -> stow_core::Result<Vec<DueReminder>> {
    let end = stow_core::ids::parse_rfc3339(now)?;
    let start = chrono::DateTime::parse_from_rfc3339(&end).map_or_else(
        |_| end.clone(),
        |t| (t - DUE_LOOKBACK).to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    );
    let mut due = reminders::between(db, &start, &end)?;
    due.reverse();
    Ok(due
        .into_iter()
        .take(MAX_DUE)
        .map(|r| DueReminder {
            title: stow_core::scheduler::target_title(db, r.target_type, &r.target_id),
            reminder_id: r.id,
        })
        .collect())
}

fn menu(app: &AppHandle, model: &TrayModel) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", "Open Stow", true, None::<&str>)?;
    // An accelerator the menu cannot display must not cost us the menu item.
    let capture = MenuItem::with_id(
        app,
        "capture",
        "Quick Capture",
        true,
        Some(model.hotkey.as_str()),
    )
    .or_else(|_| MenuItem::with_id(app, "capture", "Quick Capture", true, None::<&str>))?;
    let paste = MenuItem::with_id(
        app,
        "paste",
        "Paste Link from Clipboard",
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, Some("CmdOrCtrl+,"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit Stow", true, Some("CmdOrCtrl+Q"))?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &capture,
            &paste,
            &PredefinedMenuItem::separator(app)?,
        ],
    )?;
    let context = context_entries(model);
    if !context.is_empty() {
        for (id, label) in &context {
            menu.append(&MenuItem::with_id(app, id, label, true, None::<&str>)?)?;
        }
        menu.append(&PredefinedMenuItem::separator(app)?)?;
    }
    menu.append_items(&[&settings, &PredefinedMenuItem::separator(app)?, &quit])?;
    Ok(menu)
}

pub fn build(app: &AppHandle, hotkey: &str) -> tauri::Result<()> {
    // Monochrome template (black + alpha) so macOS tints it for light/dark menu bars.
    let icon =
        tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png")).or_else(|_| {
            app.default_window_icon()
                .cloned()
                .ok_or_else(|| tauri::Error::AssetNotFound("icon".into()))
        })?;
    let model = TrayModel {
        hotkey: hotkey.to_string(),
        ..TrayModel::default()
    };
    let tray = TrayIconBuilder::with_id("stow-tray")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Stow")
        .menu(&menu(app, &model)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                let _ = crate::commands::window::show_main_window(app);
            }
            "capture" => crate::commands::window::open_capture(app),
            "paste" => paste_from_clipboard(app.clone()),
            "settings" => {
                let _ = crate::commands::window::show_main_window(app);
                let _ = app.emit_to("main", events::OPEN_SETTINGS, events::Empty {});
            }
            STOP_RECORDING_ID => crate::commands::recordings::stop_from_tray(app.clone()),
            "quit" => app.exit(0),
            other => {
                if let Some(id) = due_reminder_id(other) {
                    open_reminder(app.clone(), id.to_string());
                }
            }
        })
        .build(app)?;
    if let Some(state) = app.try_state::<AppState>() {
        *lock(&state.tray) = Some(tray);
        *lock(&state.tray_model) = model;
    }
    // The due list needs the database, so it arrives with the first refresh.
    refresh(app);
    Ok(())
}

/// Show main at the reminder's target.
fn open_reminder(app: AppHandle, reminder_id: String) {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        match reminders::get(&state.db, &reminder_id) {
            Ok(r) => {
                let _ = crate::commands::window::navigate(
                    &app,
                    Navigate {
                        target_type: r.target_type,
                        target_id: r.target_id,
                    },
                );
            }
            Err(e) => tracing::warn!("tray reminder lookup failed: {e}"),
        }
    });
}

/// Show the new hotkey next to "Quick Capture".
pub fn rebuild_menu(app: &AppHandle, hotkey: &str) {
    if let Some(state) = app.try_state::<AppState>() {
        lock(&state.tray_model).hotkey = hotkey.to_string();
    }
    refresh(app);
}

/// Reload the due reminders and the recording flag, then rebuild the menu (off the IPC
/// thread; the menu calls hop to the main thread themselves).
pub fn refresh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let due = match due_reminders(&state.db, &stow_core::ids::now()) {
            Ok(due) => due,
            Err(e) => {
                tracing::warn!("could not load due reminders for the tray: {e}");
                Vec::new()
            }
        };
        let model = {
            let mut m = lock(&state.tray_model);
            m.due = due;
            m.recording = state.recorder.is_recording();
            m.clone()
        };
        let tray = lock(&state.tray).clone();
        if let Some(tray) = tray {
            if let Err(e) = menu(&app, &model).and_then(|m| tray.set_menu(Some(m))) {
                tracing::warn!("could not rebuild tray menu: {e}");
            }
        }
    });
}

fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!("notification failed: {e}");
    }
}

/// Save the first link in the clipboard text to the Inbox and confirm with a notification.
fn paste_from_clipboard(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let text = app.clipboard().read_text().unwrap_or_default();
        if text.trim().is_empty() {
            notify(&app, "Nothing to save", "The clipboard is empty.");
            return;
        }
        let Some(url) = stow_core::capture::extract_first_url(&text) else {
            notify(
                &app,
                "Nothing to save",
                "The clipboard doesn't contain a web link.",
            );
            return;
        };
        let stow = Stow::with_jobs(state.db.clone(), state.jobs.clone());
        match stow.capture(CaptureRequest {
            url,
            ..Default::default()
        }) {
            Ok(c) if c.created => notify(&app, "Saved to Inbox", &c.value.url),
            Ok(c) => notify(&app, "Already saved", &c.value.url),
            Err(stow_core::StowError::InvalidUrl { .. }) => notify(
                &app,
                "Nothing to save",
                "The clipboard doesn't contain a web link.",
            ),
            Err(e) => notify(&app, "Could not save", &e.to_string()),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use stow_core::models::TargetType;
    use stow_core::repo::{buckets, reminders};

    fn due(id: &str, title: &str) -> DueReminder {
        DueReminder {
            reminder_id: id.into(),
            title: title.into(),
        }
    }

    #[test]
    fn context_entries_show_three_due_then_stop() {
        let model = TrayModel {
            hotkey: "Ctrl+Cmd+S".into(),
            due: vec![
                due("a", "Read this"),
                due("b", ""),
                due("c", "x".repeat(60).as_str()),
                due("d", "hidden"),
            ],
            recording: true,
        };
        let entries = context_entries(&model);
        assert_eq!(entries.len(), 4);
        assert_eq!(
            entries[0],
            ("due:a".to_string(), "Due: Read this".to_string())
        );
        assert_eq!(
            entries[1].1, "Due: Reminder",
            "empty titles get a placeholder"
        );
        assert_eq!(entries[2].1, format!("Due: {}…", "x".repeat(40)));
        assert_eq!(
            entries[3],
            (
                STOP_RECORDING_ID.to_string(),
                "Recording… ■ Stop".to_string()
            )
        );
        assert_eq!(due_reminder_id(&entries[0].0), Some("a"));
        assert_eq!(due_reminder_id(STOP_RECORDING_ID), None);
    }

    #[test]
    fn context_entries_are_empty_when_idle() {
        assert!(context_entries(&TrayModel::default()).is_empty());
        let model = TrayModel {
            recording: true,
            ..TrayModel::default()
        };
        assert_eq!(context_entries(&model).len(), 1);
    }

    #[test]
    fn due_reminders_are_recent_newest_first_and_capped() {
        let dir = crate::testutil::TempDir::new();
        let db = Db::open(dir.path()).unwrap();
        let b = buckets::create(&db, "Reading", None).unwrap();
        let now = chrono::Utc::now();
        let at = |mins_ago: i64| {
            (now - chrono::Duration::minutes(mins_ago))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        };
        // 5 past reminders, one too old, one in the future.
        for mins in [5, 10, 20, 30, 40] {
            reminders::create(&db, TargetType::Bucket, &b.id, &at(mins), None).unwrap();
        }
        reminders::create(&db, TargetType::Bucket, &b.id, &at(25 * 60), None).unwrap();
        reminders::create(&db, TargetType::Bucket, &b.id, &at(-60), None).unwrap();
        let due = due_reminders(&db, &at(0)).unwrap();
        assert_eq!(due.len(), MAX_DUE);
        assert!(due.iter().all(|d| d.title == "Reading"));
        let newest = reminders::between(&db, &at(6), &at(4)).unwrap();
        assert_eq!(due[0].reminder_id, newest[0].id, "newest first");
    }
}
