//! Event names and payloads sent to the webviews; see the "Events" sections of
//! docs/superpowers/plans/tauri-contract.md.

use serde::Serialize;
use stow_core::models::TargetType;

pub const BUCKETS_CHANGED: &str = "buckets-changed";
pub const ITEM_ADDED: &str = "item-added";
pub const ITEM_UPDATED: &str = "item-updated";
pub const APP_WILL_QUIT: &str = "app-will-quit";
pub const RECORDING_UPDATED: &str = "recording-updated";
pub const AI_RUN_UPDATED: &str = "ai-run-updated";
pub const MODEL_DOWNLOAD_PROGRESS: &str = "model-download-progress";
pub const REMINDER_FIRED: &str = "reminder-fired";
pub const NAVIGATE: &str = "navigate";
pub const CAPTURE_OPEN: &str = "capture-open";
pub const PAIR_REQUEST: &str = "pair-request";
pub const HOTKEY_STATUS: &str = "hotkey-status";
/// Tauri-phase addition: the tray's "Settings…" item asks the main window to open Settings.
pub const OPEN_SETTINGS: &str = "open-settings";
/// Polish addition: recording started or stopped by any path (command or tray).
pub const RECORDING_STATE: &str = "recording-state";

/// `{}`
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Empty {}

/// `{id}`
#[derive(Debug, Clone, Serialize)]
pub struct Id {
    pub id: String,
}

/// `model-download-progress`; `kind` is extra (e.g. `whisper_model`).
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub kind: String,
    pub received: u64,
    pub total: Option<u64>,
}

/// `reminder-fired`
#[derive(Debug, Clone, Serialize)]
pub struct ReminderFired {
    pub reminder_id: String,
    pub target_type: TargetType,
    pub target_id: String,
    pub title: String,
}

/// `navigate`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Navigate {
    pub target_type: TargetType,
    pub target_id: String,
}

/// `recording-state`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RecordingState {
    pub recording: bool,
}

/// `pair-request`
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairRequest {
    pub id: String,
    pub name: String,
}

/// `hotkey-status`, also folded into `get_settings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HotkeyStatus {
    pub hotkey: String,
    pub registered: bool,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn event_payloads_serialize_like_contract() {
        assert_eq!(serde_json::to_value(Empty {}).unwrap(), json!({}));
        assert_eq!(
            serde_json::to_value(Id { id: "a".into() }).unwrap(),
            json!({"id": "a"})
        );
        assert_eq!(
            serde_json::to_value(Progress {
                kind: "whisper_model".into(),
                received: 5,
                total: None
            })
            .unwrap(),
            json!({"kind": "whisper_model", "received": 5, "total": null})
        );
        assert_eq!(
            serde_json::to_value(ReminderFired {
                reminder_id: "r".into(),
                target_type: TargetType::Task,
                target_id: "t".into(),
                title: "T".into()
            })
            .unwrap(),
            json!({"reminder_id": "r", "target_type": "task", "target_id": "t", "title": "T"})
        );
        assert_eq!(
            serde_json::to_value(Navigate {
                target_type: TargetType::Item,
                target_id: "i".into()
            })
            .unwrap(),
            json!({"target_type": "item", "target_id": "i"})
        );
        assert_eq!(
            serde_json::to_value(RecordingState { recording: true }).unwrap(),
            json!({"recording": true})
        );
        assert_eq!(
            serde_json::to_value(PairRequest {
                id: "p".into(),
                name: "Chrome".into()
            })
            .unwrap(),
            json!({"id": "p", "name": "Chrome"})
        );
        assert_eq!(
            serde_json::to_value(HotkeyStatus {
                hotkey: "Ctrl+Cmd+S".into(),
                registered: false,
                error: Some("taken".into())
            })
            .unwrap(),
            json!({"hotkey": "Ctrl+Cmd+S", "registered": false, "error": "taken"})
        );
    }
}
