use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use stow_core::ai::{
    self,
    keys::{self, Account},
};
use stow_core::jobs::Job;
use stow_core::models::{AiAction, AiRun, ParentType};
use stow_core::repo::{
    ai_runs,
    settings::{self, Key},
};
use tauri::State;

use super::db;
use crate::error::{blocking, require_id, CmdResult};
use crate::state::AppState;

#[tauri::command]
pub async fn list_ai_runs(
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
) -> CmdResult<Vec<AiRun>> {
    require_id("parent_id", &parent_id)?;
    db(&state, move |d| ai_runs::list(d, parent_type, &parent_id)).await
}

/// Create a pending run and queue it. The result arrives via the `ai-run-updated` event.
#[tauri::command]
pub async fn run_ai(
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
    action: AiAction,
) -> CmdResult<AiRun> {
    require_id("parent_id", &parent_id)?;
    let run = db(&state, move |d| {
        let provider = settings::validate_ai_provider(&settings::get_key(d, Key::AiProvider)?)?;
        let model = Some(settings::get_key(d, Key::AiModel)?)
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| ai::default_model(provider).to_string());
        let ctx = ai::load_context(d, parent_type, &parent_id)?;
        let mut h = DefaultHasher::new();
        action.as_str().hash(&mut h);
        parent_id.hash(&mut h);
        format!("{ctx:?}").hash(&mut h);
        ai_runs::create(
            d,
            parent_type,
            &parent_id,
            action,
            provider,
            &model,
            &format!("{:016x}", h.finish()),
        )
    })
    .await?;
    state.jobs.enqueue(Job::Ai {
        run_id: run.id.clone(),
    });
    Ok(run)
}

#[tauri::command]
pub async fn set_api_key(account: String, key: String) -> CmdResult<()> {
    let account = account.parse::<Account>()?;
    blocking(move || keys::set(account, key.trim())).await
}

#[tauri::command]
pub async fn has_api_key(account: String) -> CmdResult<bool> {
    let account = account.parse::<Account>()?;
    blocking(move || keys::get(account).map(|k| k.is_some())).await
}

#[tauri::command]
pub async fn delete_api_key(account: String) -> CmdResult<()> {
    let account = account.parse::<Account>()?;
    blocking(move || keys::delete(account)).await
}
