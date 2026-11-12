use stow_core::models::SearchHit;
use stow_core::repo::search;
use tauri::State;

use super::db;
use crate::error::CmdResult;
use crate::state::AppState;

#[tauri::command]
pub async fn search(state: State<'_, AppState>, q: String) -> CmdResult<Vec<SearchHit>> {
    db(&state, move |d| search::query(d, &q, 50)).await
}
