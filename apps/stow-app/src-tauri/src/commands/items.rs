use serde::Serialize;
use stow_core::jobs::Job;
use stow_core::models::{Item, JobStatus};
use stow_core::repo::items::{self, ItemFilter};
use stow_core::{CaptureRequest, Created, Stow};
use tauri::{AppHandle, State};

use super::db;
use crate::error::{blocking, require_id, CmdError, CmdResult};
use crate::state::AppState;

/// `add_item` result.
#[derive(Debug, Serialize)]
pub struct AddedItem {
    pub item: Item,
    pub created: bool,
}

/// Save a URL on the blocking pool. Jobs are enqueued only for new items.
pub async fn add(
    state: &AppState,
    url: String,
    bucket_id: Option<String>,
    title: Option<String>,
) -> CmdResult<Created<Item>> {
    if let Some(b) = &bucket_id {
        require_id("bucket_id", b)?;
    }
    let stow = Stow::with_jobs(state.db.clone(), state.jobs.clone());
    blocking(move || {
        stow.capture(CaptureRequest {
            url,
            bucket_id,
            title,
        })
    })
    .await
}

#[tauri::command]
pub async fn list_items(
    state: State<'_, AppState>,
    filter: ItemFilter,
    include_done: Option<bool>,
) -> CmdResult<Vec<Item>> {
    if let ItemFilter::Bucket(id) = &filter {
        require_id("filter.id", id)?;
    }
    db(&state, move |d| {
        items::list(d, filter, include_done.unwrap_or(false))
    })
    .await
}

#[tauri::command]
pub async fn get_item(state: State<'_, AppState>, id: String) -> CmdResult<Item> {
    require_id("id", &id)?;
    db(&state, move |d| items::get(d, &id)).await
}

#[tauri::command]
pub async fn add_item(
    state: State<'_, AppState>,
    url: String,
    bucket_id: Option<String>,
    title: Option<String>,
) -> CmdResult<AddedItem> {
    let c = add(&state, url, bucket_id, title).await?;
    Ok(AddedItem {
        item: c.value,
        created: c.created,
    })
}

#[tauri::command]
pub async fn move_item(
    state: State<'_, AppState>,
    id: String,
    bucket_id: Option<String>,
) -> CmdResult<Item> {
    require_id("id", &id)?;
    if let Some(b) = &bucket_id {
        require_id("bucket_id", b)?;
    }
    db(&state, move |d| {
        items::move_to(d, &id, bucket_id.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn update_item_notes(
    state: State<'_, AppState>,
    id: String,
    notes_md: String,
) -> CmdResult<Item> {
    require_id("id", &id)?;
    db(&state, move |d| items::set_notes(d, &id, &notes_md)).await
}

#[tauri::command]
pub async fn set_item_done(state: State<'_, AppState>, id: String, done: bool) -> CmdResult<Item> {
    require_id("id", &id)?;
    db(&state, move |d| items::set_done(d, &id, done)).await
}

#[tauri::command]
pub async fn delete_item(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    require_id("id", &id)?;
    db(&state, move |d| items::delete(d, &id)).await?;
    // Its reminders went with it; the tray's "Due:" entries must not name them.
    crate::tray::refresh(&app);
    Ok(())
}

/// Queue a fresh fetch. The row is marked `pending` (attempts reset) BEFORE the job is
/// queued so a crash in between leaves work `resume_pending` picks up. When the same job
/// is already in flight the result is `busy`; the running job overwrites the status when
/// it finishes, so nothing is reverted.
#[tauri::command]
pub async fn refetch_item(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    require_id("id", &id)?;
    let item_id = id.clone();
    db(&state, move |d| requeue_snapshot(d, &item_id)).await?;
    if !state.jobs.enqueue(Job::FetchSnapshot {
        item_id: id.clone(),
    }) {
        return Err(CmdError::busy("That page is already being fetched."));
    }
    state.jobs.enqueue(Job::FetchMetadata { item_id: id });
    Ok(())
}

/// `snapshot_status = pending`, error cleared, attempts back to 0 (`not_found` for an
/// unknown id).
pub fn requeue_snapshot(db: &stow_core::Db, id: &str) -> stow_core::Result<()> {
    items::set_snapshot_status(db, id, JobStatus::Pending, None)?;
    db.tx(|t| {
        t.execute("UPDATE items SET snapshot_attempts = 0 WHERE id = ?1", [id])?;
        Ok(())
    })
}

#[tauri::command]
pub async fn read_snapshot(state: State<'_, AppState>, id: String) -> CmdResult<Option<String>> {
    require_id("id", &id)?;
    let dir = state.db.data_dir().to_path_buf();
    blocking(move || stow_core::fetch::snapshot::read_snapshot_md(&dir, &id)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requeue_snapshot_resets_status_and_attempts() {
        let dir = crate::testutil::TempDir::new();
        let db = stow_core::Db::open(dir.path()).unwrap();
        let it = items::create(
            &db,
            items::NewItem {
                url: "https://example.com/a".into(),
                bucket_id: None,
                title: None,
            },
        )
        .unwrap()
        .value;
        items::set_snapshot_status(&db, &it.id, JobStatus::Failed, Some("boom")).unwrap();
        db.tx(|t| {
            t.execute(
                "UPDATE items SET snapshot_attempts = 3 WHERE id = ?1",
                [&it.id],
            )?;
            Ok(())
        })
        .unwrap();
        requeue_snapshot(&db, &it.id).unwrap();
        let again = items::get(&db, &it.id).unwrap();
        assert_eq!(again.snapshot_status, JobStatus::Pending);
        assert_eq!(again.snapshot_error, None);
        let attempts: i64 = db
            .tx(|t| {
                Ok(t.query_row(
                    "SELECT snapshot_attempts FROM items WHERE id = ?1",
                    [&it.id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(attempts, 0);
        assert_eq!(
            requeue_snapshot(&db, &stow_core::ids::new_id())
                .unwrap_err()
                .code(),
            "not_found"
        );
    }
}
