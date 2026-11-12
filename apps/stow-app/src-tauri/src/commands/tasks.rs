use stow_core::models::{ParentType, Task};
use stow_core::repo::tasks;
use tauri::{AppHandle, State};

use super::db;
use crate::error::{require_id, CmdResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_tasks(
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
) -> CmdResult<Vec<Task>> {
    require_id("parent_id", &parent_id)?;
    db(&state, move |d| tasks::list(d, parent_type, &parent_id)).await
}

#[tauri::command]
pub async fn get_task(state: State<'_, AppState>, id: String) -> CmdResult<Task> {
    require_id("id", &id)?;
    db(&state, move |d| tasks::get(d, &id)).await
}

#[tauri::command]
pub async fn add_task(
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
    title: String,
) -> CmdResult<Task> {
    require_id("parent_id", &parent_id)?;
    db(&state, move |d| {
        tasks::create(d, parent_type, &parent_id, &title)
    })
    .await
}

#[tauri::command]
pub async fn toggle_task(state: State<'_, AppState>, id: String) -> CmdResult<Task> {
    require_id("id", &id)?;
    db(&state, move |d| tasks::toggle(d, &id)).await
}

#[tauri::command]
pub async fn rename_task(state: State<'_, AppState>, id: String, title: String) -> CmdResult<Task> {
    require_id("id", &id)?;
    db(&state, move |d| tasks::rename(d, &id, &title)).await
}

#[tauri::command]
pub async fn delete_task(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    require_id("id", &id)?;
    db(&state, move |d| tasks::delete(d, &id)).await?;
    // Its reminders went with it; the tray's "Due:" entries must not name them.
    crate::tray::refresh(&app);
    Ok(())
}
