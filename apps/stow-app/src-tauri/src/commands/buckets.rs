use stow_core::models::Bucket;
use stow_core::repo::buckets::{self, BucketPatch};
use tauri::{AppHandle, State};

use super::db;
use crate::error::{require_id, CmdResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_buckets(state: State<'_, AppState>) -> CmdResult<Vec<Bucket>> {
    db(&state, buckets::list).await
}

fn changed(state: &AppState) {
    state.jobs.notify(stow_core::jobs::JobEvent::BucketsChanged);
}

#[tauri::command]
pub async fn create_bucket(
    state: State<'_, AppState>,
    name: String,
    emoji: Option<String>,
) -> CmdResult<Bucket> {
    let b = db(&state, move |d| buckets::create(d, &name, emoji.as_deref())).await?;
    changed(&state);
    Ok(b)
}

#[tauri::command]
pub async fn update_bucket(
    state: State<'_, AppState>,
    id: String,
    patch: BucketPatch,
) -> CmdResult<Bucket> {
    require_id("id", &id)?;
    let b = db(&state, move |d| buckets::update(d, &id, &patch)).await?;
    changed(&state);
    Ok(b)
}

#[tauri::command]
pub async fn delete_bucket(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<()> {
    require_id("id", &id)?;
    db(&state, move |d| buckets::delete(d, &id)).await?;
    changed(&state);
    // Its reminders went with it; the tray's "Due:" entries must not name them.
    crate::tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub async fn reorder_buckets(state: State<'_, AppState>, ids: Vec<String>) -> CmdResult<()> {
    for id in &ids {
        require_id("ids", id)?;
    }
    db(&state, move |d| buckets::reorder(d, &ids)).await?;
    changed(&state);
    Ok(())
}
