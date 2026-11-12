//! Every 30 s: fire due reminders as macOS notifications and tell the UI.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use stow_core::models::TargetType;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::events::{self, Navigate, ReminderFired};
use crate::state::{lock, AppState};

/// Reopening the app this soon after a notification counts as clicking it (the fallback
/// for when the notification plugin reports no `tap`, e.g. the alert was dismissed and
/// the app opened from the dock).
pub const REOPEN_WINDOW: Duration = Duration::from_secs(120);

/// Keys of the notification `extra` payload that carry the reminder's target.
const EXTRA_TARGET_TYPE: &str = "target_type";
const EXTRA_TARGET_ID: &str = "target_id";

/// Where a notification click should take the user: the target stored in the
/// notification's `extra` when the action is `tap` (a click on the notification itself).
pub fn tap_target(action_id: &str, extra: &HashMap<String, serde_json::Value>) -> Option<Navigate> {
    if action_id != "tap" {
        return None;
    }
    let target_type: TargetType =
        serde_json::from_value(extra.get(EXTRA_TARGET_TYPE)?.clone()).ok()?;
    let target_id = extra.get(EXTRA_TARGET_ID)?.as_str()?.to_string();
    stow_core::ids::require_ulid("target_id", &target_id).ok()?;
    Some(Navigate {
        target_type,
        target_id,
    })
}

/// Route notification clicks to their reminder's target. Call once at setup.
pub fn listen_for_clicks(app: &AppHandle) {
    let handle = app.clone();
    let r = app.notification().on_action(move |performed| {
        let Some(n) = performed.notification() else {
            return;
        };
        if let Some(target) = tap_target(performed.action_id(), n.extra()) {
            if let Some(state) = handle.try_state::<AppState>() {
                // The click already navigated; a dock click later must not repeat it.
                *lock(&state.last_fired) = None;
            }
            let _ = crate::commands::window::navigate(&handle, target);
        }
    });
    if let Err(e) = r {
        tracing::warn!("notification clicks unavailable: {e}");
    }
}

/// Notification body: the reminder's message, else the target's title.
pub fn notification_body(message: Option<&str>, title: &str) -> String {
    message
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .unwrap_or(title)
        .to_string()
}

/// The target of a reminder fired within `REOPEN_WINDOW`, consumed once.
pub fn take_recent(slot: &mut Option<(Instant, Navigate)>, now: Instant) -> Option<Navigate> {
    match slot.take() {
        Some((at, target)) if now.duration_since(at) <= REOPEN_WINDOW => Some(target),
        _ => None,
    }
}

pub fn start(app: AppHandle, db: Arc<stow_core::Db>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            let now = stow_core::ids::now();
            let app2 = app.clone();
            let db2 = db.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                stow_core::scheduler::fire_due(&db2, &now, &mut |r, title| {
                    let body = notification_body(r.message.as_deref(), title);
                    let heading = if r.message.as_deref().is_some_and(|m| !m.trim().is_empty()) {
                        title
                    } else {
                        "Reminder"
                    };
                    if let Err(e) = app2
                        .notification()
                        .builder()
                        .title(heading)
                        .body(&body)
                        .extra(EXTRA_TARGET_TYPE, r.target_type)
                        .extra(EXTRA_TARGET_ID, &r.target_id)
                        .show()
                    {
                        tracing::warn!("notification failed: {e}");
                    }
                    if let Some(state) = app2.try_state::<AppState>() {
                        *lock(&state.last_fired) = Some((
                            Instant::now(),
                            Navigate {
                                target_type: r.target_type,
                                target_id: r.target_id.clone(),
                            },
                        ));
                    }
                    let _ = app2.emit(
                        events::REMINDER_FIRED,
                        ReminderFired {
                            reminder_id: r.id.clone(),
                            target_type: r.target_type,
                            target_id: r.target_id.clone(),
                            title: title.to_string(),
                        },
                    );
                })
            })
            .await;
            match result {
                Ok(Ok(fired)) if !fired.is_empty() => {
                    tracing::info!(count = fired.len(), "reminders fired");
                    crate::tray::refresh(&app);
                }
                Ok(Err(e)) => tracing::warn!("scheduler tick failed: {e}"),
                Err(e) => tracing::warn!("scheduler task panicked: {e}"),
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use stow_core::models::TargetType;

    #[test]
    fn notification_body_defaults_to_target_title() {
        assert_eq!(notification_body(None, "Read this"), "Read this");
        assert_eq!(notification_body(Some("  "), "Read this"), "Read this");
        assert_eq!(notification_body(Some("Call Bob"), "Read this"), "Call Bob");
    }

    #[test]
    fn reopen_navigates_only_soon_after_firing_and_once() {
        let t0 = Instant::now();
        let target = Navigate {
            target_type: TargetType::Item,
            target_id: "x".into(),
        };
        let mut slot = Some((t0, target.clone()));
        assert_eq!(
            take_recent(&mut slot, t0 + Duration::from_secs(5)),
            Some(target.clone())
        );
        assert_eq!(
            take_recent(&mut slot, t0 + Duration::from_secs(6)),
            None,
            "consumed"
        );
        let mut slot = Some((t0, target.clone()));
        assert_eq!(
            take_recent(&mut slot, t0 + Duration::from_secs(119)),
            Some(target.clone())
        );
        let mut slot = Some((t0, target));
        assert_eq!(take_recent(&mut slot, t0 + Duration::from_secs(121)), None);
        assert!(slot.is_none());
    }

    #[test]
    fn tap_target_reads_the_notification_extra() {
        use serde_json::json;
        let id = stow_core::ids::new_id();
        let extra: HashMap<String, serde_json::Value> = HashMap::from([
            ("target_type".into(), json!("item")),
            ("target_id".into(), json!(id)),
        ]);
        assert_eq!(
            tap_target("tap", &extra),
            Some(Navigate {
                target_type: TargetType::Item,
                target_id: id.clone()
            })
        );
        assert_eq!(
            tap_target("snooze", &extra),
            None,
            "only a click on the body"
        );
        assert_eq!(tap_target("tap", &HashMap::new()), None);
        let bad: HashMap<String, serde_json::Value> = HashMap::from([
            ("target_type".into(), json!("item")),
            ("target_id".into(), json!("nope")),
        ]);
        assert_eq!(tap_target("tap", &bad), None, "ids are validated");
    }
}
