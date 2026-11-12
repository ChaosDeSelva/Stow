use serde::Serialize;
use stow_core::ids::parse_rfc3339;
use stow_core::models::{Reminder, TargetType};
use stow_core::repo::reminders;
use tauri::{AppHandle, State};

use super::db;
use crate::error::{require_id, CmdResult};
use crate::state::AppState;

#[derive(Serialize)]
pub struct ReminderAgenda {
    pub overdue: Vec<Reminder>,
    pub today: Vec<Reminder>,
    pub upcoming: Vec<Reminder>,
}

fn effective(r: &Reminder) -> &str {
    r.snoozed_until.as_deref().unwrap_or(&r.fire_at)
}

#[tauri::command]
pub async fn list_reminders(
    state: State<'_, AppState>,
    target_type: TargetType,
    target_id: String,
) -> CmdResult<Vec<Reminder>> {
    require_id("target_id", &target_id)?;
    db(&state, move |d| {
        reminders::list_for(d, target_type, &target_id)
    })
    .await
}

#[tauri::command]
pub async fn add_reminder(
    state: State<'_, AppState>,
    target_type: TargetType,
    target_id: String,
    fire_at: String,
    message: Option<String>,
) -> CmdResult<Reminder> {
    require_id("target_id", &target_id)?;
    let fire_at = parse_rfc3339(&fire_at)?;
    db(&state, move |d| {
        reminders::create(d, target_type, &target_id, &fire_at, message.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn snooze_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    until: String,
) -> CmdResult<Reminder> {
    require_id("id", &id)?;
    let until = parse_rfc3339(&until)?;
    let r = db(&state, move |d| reminders::snooze(d, &id, &until)).await?;
    crate::tray::refresh(&app);
    Ok(r)
}

#[tauri::command]
pub async fn delete_reminder(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<()> {
    require_id("id", &id)?;
    db(&state, move |d| reminders::delete(d, &id)).await?;
    crate::tray::refresh(&app);
    Ok(())
}

/// Overdue = unfired before `start`; today = effective time in [start, end); upcoming = unfired after `end`.
#[tauri::command]
pub async fn reminder_agenda(
    state: State<'_, AppState>,
    start: String,
    end: String,
) -> CmdResult<ReminderAgenda> {
    let start = parse_rfc3339(&start)?;
    let end = parse_rfc3339(&end)?;
    db(&state, move |d| {
        let overdue = reminders::due(d, &start)?;
        let today = reminders::between(d, &start, &end)?;
        let upcoming = reminders::list_upcoming(d, 200)?
            .into_iter()
            .filter(|r| effective(r) >= end.as_str())
            .take(50)
            .collect();
        Ok(ReminderAgenda {
            overdue,
            today,
            upcoming,
        })
    })
    .await
}
