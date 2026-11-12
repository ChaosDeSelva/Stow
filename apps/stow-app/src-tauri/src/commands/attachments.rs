use std::path::PathBuf;

use stow_core::models::{Attachment, ParentType};
use stow_core::repo::attachments;
use tauri::State;

use super::db;
use crate::error::{require_id, CmdResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_attachments(
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
) -> CmdResult<Vec<Attachment>> {
    require_id("parent_id", &parent_id)?;
    db(&state, move |d| {
        attachments::list(d, parent_type, &parent_id)
    })
    .await
}

/// Copies the file into the data dir, so it runs on the blocking pool.
#[tauri::command]
pub async fn add_attachment(
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
    src_path: String,
) -> CmdResult<Attachment> {
    require_id("parent_id", &parent_id)?;
    db(&state, move |d| {
        attachments::add(d, parent_type, &parent_id, &PathBuf::from(src_path))
    })
    .await
}

#[tauri::command]
pub async fn delete_attachment(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    require_id("id", &id)?;
    db(&state, move |d| attachments::delete(d, &id)).await
}
