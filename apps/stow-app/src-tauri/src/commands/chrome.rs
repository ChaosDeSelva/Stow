//! Chrome commands run `AppleScript`, which blocks, so they use the blocking pool.

use serde::Serialize;
use tauri::State;

use crate::chrome::{self, ChromeTab};
use crate::error::{blocking, require_id, CmdResult};
use crate::state::AppState;

#[derive(Serialize)]
pub struct ImportResult {
    pub imported: usize,
    pub skipped: usize,
}

#[derive(Serialize)]
pub struct Closed {
    pub closed: bool,
}

#[tauri::command]
pub async fn capture_front_tab() -> CmdResult<ChromeTab> {
    blocking(chrome::front_tab).await
}

#[tauri::command]
pub async fn close_front_tab(expected_url: Option<String>) -> CmdResult<Closed> {
    let closed = blocking(move || chrome::close_front_tab(expected_url.as_deref())).await?;
    Ok(Closed { closed })
}

#[tauri::command]
pub async fn list_chrome_tabs() -> CmdResult<Vec<ChromeTab>> {
    blocking(chrome::all_tabs).await
}

/// Save the given tabs. Duplicates and non-http URLs count as skipped.
#[tauri::command]
pub async fn import_chrome_tabs(
    state: State<'_, AppState>,
    tabs: Vec<ChromeTab>,
    bucket_id: Option<String>,
    close: bool,
) -> CmdResult<ImportResult> {
    if let Some(b) = &bucket_id {
        require_id("bucket_id", b)?;
    }
    let mut imported = 0;
    let mut skipped = 0;
    let mut saved: Vec<ChromeTab> = Vec::new();
    for tab in tabs {
        let title = Some(tab.title.clone()).filter(|t| !t.trim().is_empty());
        match super::items::add(&state, tab.url.clone(), bucket_id.clone(), title).await {
            Ok(c) => {
                if c.created {
                    imported += 1;
                } else {
                    skipped += 1;
                }
                saved.push(tab);
            }
            Err(_) => skipped += 1,
        }
    }
    if close && !saved.is_empty() {
        blocking(move || chrome::close_tabs(&saved)).await?;
    }
    Ok(ImportResult { imported, skipped })
}
