use std::time::Duration;

use serde::Serialize;
use stow_core::audio::MAX_RECORDING;
use stow_core::ids::new_id;
use stow_core::jobs::{Job, JobEvent};
use stow_core::models::{JobStatus, ParentType, Recording};
use stow_core::repo::{recordings, settings};
use stow_core::transcribe;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use super::db;
use crate::error::{blocking, require_id, CmdError, CmdResult};
use crate::events::{self, RecordingState};
use crate::state::{lock, AppState};

/// `start_recording` while a recording is in progress is `busy`, not an `audio` failure.
pub fn reject_if_recording(recording: bool) -> CmdResult<()> {
    if recording {
        Err(CmdError::busy("Already recording."))
    } else {
        Ok(())
    }
}

/// Tell every window and the tray that recording started or stopped.
fn publish_state(app: &AppHandle, recording: bool) {
    let _ = app.emit(events::RECORDING_STATE, RecordingState { recording });
    crate::tray::refresh(app);
}

#[tauri::command]
pub async fn list_recordings(
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
) -> CmdResult<Vec<Recording>> {
    require_id("parent_id", &parent_id)?;
    db(&state, move |d| {
        recordings::list(d, parent_type, &parent_id)
    })
    .await
}

#[tauri::command]
#[allow(clippy::unnecessary_wraps)] // every command returns CmdResult for a uniform IPC shape
pub fn is_recording(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.recorder.is_recording())
}

/// Start recording. The parent given here wins over whatever is passed to `stop_recording`,
/// so switching items mid-recording never misattributes the audio.
#[tauri::command]
pub async fn start_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    parent_type: Option<ParentType>,
    parent_id: Option<String>,
) -> CmdResult<()> {
    if let Some(id) = &parent_id {
        require_id("parent_id", id)?;
    }
    reject_if_recording(state.recorder.is_recording())?;
    let path = state
        .db
        .data_dir()
        .join("audio")
        .join(format!("tmp-{}.wav", new_id()));
    let recorder = state.recorder.clone();
    blocking(move || recorder.start(path)).await?;
    *lock(&state.recording_parent) = parent_type.zip(parent_id);
    publish_state(&app, true);
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    parent_type: ParentType,
    parent_id: String,
) -> CmdResult<Recording> {
    require_id("parent_id", &parent_id)?;
    finish(&app, &state, Some((parent_type, parent_id))).await
}

/// Stop the recorder and file the WAV under the parent remembered at start (else
/// `fallback`). A dead recorder thread surfaces as an `audio` error from `stop`.
/// Shared by the command and the tray's "Stop" item.
pub async fn finish(
    app: &AppHandle,
    state: &AppState,
    fallback: Option<(ParentType, String)>,
) -> CmdResult<Recording> {
    let recorder = state.recorder.clone();
    let info = blocking(move || recorder.stop()).await;
    publish_state(app, false);
    let info = info?;
    let remembered = lock(&state.recording_parent).take();
    let Some((parent_type, parent_id)) = remembered.or(fallback) else {
        return Err(CmdError::new(
            "audio",
            format!(
                "no item to attach the recording to (kept at {})",
                info.path.display()
            ),
        ));
    };
    let db = state.db.clone();
    let rec = blocking(move || save_recording(&db, &info, parent_type, &parent_id)).await?;
    state.jobs.enqueue(Job::Transcribe {
        recording_id: rec.id.clone(),
    });
    Ok(rec)
}

/// Move the stopped recording's WAV to its final name and insert the row. On any failure
/// the audio is never lost: the error names where the WAV was kept.
pub fn save_recording(
    db: &stow_core::Db,
    info: &stow_core::audio::RecordingInfo,
    parent_type: ParentType,
    parent_id: &str,
) -> CmdResult<Recording> {
    let final_path = db
        .data_dir()
        .join("audio")
        .join(format!("{}.wav", new_id()));
    std::fs::rename(&info.path, &final_path)
        .map_err(|e| kept_at("save the recording", &info.path, &e))?;
    let duration = i64::try_from(info.duration_ms).unwrap_or(i64::MAX);
    recordings::create(
        db,
        parent_type,
        parent_id,
        &final_path.to_string_lossy(),
        duration,
    )
    .map_err(|e| kept_at("add the recording to the library", &final_path, &e))
}

/// `io` error for a step that failed after the WAV was written: `path` is where it is.
pub fn kept_at(what: &str, path: &std::path::Path, e: &dyn std::fmt::Display) -> CmdError {
    CmdError::new(
        "io",
        format!("could not {what} (audio kept at {}): {e}", path.display()),
    )
}

/// At exit: stop a recording in progress and file it under the parent it was started
/// for, so quitting mid-recording loses nothing. Without a parent the temporary WAV
/// stays in `audio/` and is logged.
pub fn save_on_exit(state: &AppState) {
    if !state.recorder.is_recording() {
        return;
    }
    let info = match state.recorder.stop() {
        Ok(info) => info,
        Err(e) => {
            tracing::warn!("could not stop the recording at exit: {e}");
            return;
        }
    };
    let Some((parent_type, parent_id)) = lock(&state.recording_parent).take() else {
        tracing::warn!(
            path = %info.path.display(),
            "recording stopped at exit had no item; WAV kept"
        );
        return;
    };
    match save_recording(&state.db, &info, parent_type, &parent_id) {
        Ok(rec) => tracing::info!(id = %rec.id, "recording saved at exit"),
        Err(e) => tracing::warn!("recording not saved at exit: {}", e.message),
    }
}

/// Notification body when the recorder stops itself: "Recording stopped after 3 hours".
pub fn auto_stop_message(limit: Duration) -> String {
    let secs = limit.as_secs();
    let (n, unit) = if secs >= 3600 {
        (secs / 3600, "hour")
    } else if secs >= 60 {
        (secs / 60, "minute")
    } else {
        (secs, "second")
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("Recording stopped after {n} {unit}{plural}")
}

/// The recorder hit [`MAX_RECORDING`] and finalised the WAV: tell the UI and the tray,
/// say why in a notification, and file the recording under the parent remembered at
/// start (like `finish`). Runs on the recorder thread, so the work is spawned.
pub fn on_auto_stop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        publish_state(&app, false);
        let info = match state.recorder.take_auto_stopped() {
            Some(Ok(info)) => info,
            Some(Err(e)) => {
                tracing::warn!("recording hit the limit but could not be finalised: {e}");
                return;
            }
            None => {
                tracing::warn!("auto-stop hook ran with no recording to file");
                return;
            }
        };
        let body = auto_stop_message(MAX_RECORDING);
        if let Err(e) = app
            .notification()
            .builder()
            .title("Recording stopped")
            .body(&body)
            .show()
        {
            tracing::warn!("notification failed: {e}");
        }
        let Some((parent_type, parent_id)) = lock(&state.recording_parent).take() else {
            tracing::warn!(
                path = %info.path.display(),
                "recording stopped at the limit had no item; WAV kept"
            );
            return;
        };
        let db = state.db.clone();
        match blocking(move || save_recording(&db, &info, parent_type, &parent_id)).await {
            Ok(rec) => {
                state.jobs.enqueue(Job::Transcribe {
                    recording_id: rec.id.clone(),
                });
                let _ = app.emit(events::RECORDING_UPDATED, events::Id { id: rec.id });
            }
            Err(e) => tracing::warn!("recording not saved after the limit: {}", e.message),
        }
    });
}

/// The tray's "Stop" item: file the recording under the parent it was started for.
pub fn stop_from_tray(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        match finish(&app, &state, None).await {
            Ok(rec) => {
                let _ = app.emit(events::RECORDING_UPDATED, events::Id { id: rec.id });
            }
            Err(e) => tracing::warn!("stop from tray failed: {}", e.message),
        }
    });
}

#[tauri::command]
pub async fn delete_recording(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    require_id("id", &id)?;
    db(&state, move |d| recordings::delete(d, &id)).await
}

/// Like `refetch_item`: the row goes `pending` (attempts reset) before the job is
/// queued; a duplicate in flight is `busy` and nothing is reverted.
#[tauri::command]
pub async fn retranscribe(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    require_id("id", &id)?;
    let rid = id.clone();
    db(&state, move |d| requeue_transcription(d, &rid)).await?;
    if !state.jobs.enqueue(Job::Transcribe { recording_id: id }) {
        return Err(CmdError::busy(
            "That recording is already being transcribed.",
        ));
    }
    Ok(())
}

/// `transcript_status = pending`, error cleared, attempts back to 0 (`not_found` for an
/// unknown id).
pub fn requeue_transcription(db: &stow_core::Db, id: &str) -> stow_core::Result<()> {
    recordings::set_status(db, id, JobStatus::Pending, None)?;
    db.tx(|t| {
        t.execute(
            "UPDATE recordings SET transcript_attempts = 0 WHERE id = ?1",
            [id],
        )?;
        Ok(())
    })
}

#[tauri::command]
pub async fn whisper_model_downloaded(
    state: State<'_, AppState>,
    model: String,
) -> CmdResult<bool> {
    let model = settings::validate_whisper_model(&model)?.to_string();
    let path = transcribe::model_path(state.db.data_dir(), &model);
    blocking(move || Ok::<_, CmdError>(path.is_file())).await
}

#[derive(Debug, Serialize)]
pub struct ModelPath {
    pub path: String,
}

/// Download the model if missing; progress arrives as `model-download-progress`.
#[tauri::command]
pub async fn download_whisper_model(
    state: State<'_, AppState>,
    model: String,
) -> CmdResult<ModelPath> {
    let model = settings::validate_whisper_model(&model)?.to_string();
    let dir = state.db.data_dir().to_path_buf();
    let jobs = state.jobs.clone();
    // Its own task so the download never runs on (or blocks) the IPC thread.
    let path = tauri::async_runtime::spawn(async move {
        let client = transcribe::download_client()?;
        transcribe::ensure_model(&client, &dir, &model, move |received, total| {
            jobs.notify(JobEvent::Progress {
                kind: "whisper_model".into(),
                received,
                total,
            });
        })
        .await
    })
    .await
    .map_err(|e| CmdError::internal(format!("download task failed: {e}")))??;
    Ok(ModelPath {
        path: path.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starting_twice_is_busy_and_a_dead_recorder_is_audio() {
        assert!(reject_if_recording(false).is_ok());
        assert_eq!(reject_if_recording(true).unwrap_err().code, "busy");
        // `RecorderHandle` reports a vanished worker thread as `StowError::Audio`, which
        // `finish` propagates unchanged.
        let e: CmdError = stow_core::StowError::Audio("recorder thread stopped".into()).into();
        assert_eq!(e.code, "audio");
    }

    #[test]
    fn save_recording_moves_the_wav_and_errors_name_the_kept_path() {
        let dir = crate::testutil::TempDir::new();
        let db = stow_core::Db::open(dir.path()).unwrap();
        let b = stow_core::repo::buckets::create(&db, "B", None).unwrap();
        let tmp = dir.path().join("audio").join("tmp-x.wav");
        std::fs::write(&tmp, b"RIFF").unwrap();
        let info = stow_core::audio::RecordingInfo {
            path: tmp.clone(),
            duration_ms: 1200,
        };
        let rec = save_recording(&db, &info, ParentType::Bucket, &b.id).unwrap();
        assert!(!tmp.exists(), "renamed away");
        assert!(std::path::Path::new(&rec.audio_path).is_file());
        assert_eq!(rec.duration_ms, 1200);
        assert_eq!(rec.transcript_status, JobStatus::Pending);
        // The source is gone now: the error is `io` and names the temp path.
        let e = save_recording(&db, &info, ParentType::Bucket, &b.id).unwrap_err();
        assert_eq!(e.code, "io");
        assert!(e.message.contains("tmp-x.wav"), "{}", e.message);
        let e = kept_at("add the recording to the library", &tmp, &"db locked");
        assert_eq!(e.code, "io");
        assert!(
            e.message
                .ends_with(&format!("(audio kept at {}): db locked", tmp.display())),
            "{}",
            e.message
        );
    }

    #[test]
    fn auto_stop_message_names_the_limit() {
        assert_eq!(
            auto_stop_message(MAX_RECORDING),
            "Recording stopped after 3 hours"
        );
        assert_eq!(
            auto_stop_message(Duration::from_secs(3600)),
            "Recording stopped after 1 hour"
        );
        assert_eq!(
            auto_stop_message(Duration::from_secs(90)),
            "Recording stopped after 1 minute"
        );
        assert_eq!(
            auto_stop_message(Duration::from_secs(45)),
            "Recording stopped after 45 seconds"
        );
    }

    #[test]
    fn requeue_transcription_resets_status_and_attempts() {
        let dir = crate::testutil::TempDir::new();
        let db = stow_core::Db::open(dir.path()).unwrap();
        let b = stow_core::repo::buckets::create(&db, "B", None).unwrap();
        let rec = recordings::create(&db, ParentType::Bucket, &b.id, "/a.wav", 1).unwrap();
        recordings::set_status(&db, &rec.id, JobStatus::Failed, Some("x")).unwrap();
        db.tx(|t| {
            t.execute(
                "UPDATE recordings SET transcript_attempts = 2 WHERE id = ?1",
                [&rec.id],
            )?;
            Ok(())
        })
        .unwrap();
        requeue_transcription(&db, &rec.id).unwrap();
        let r = recordings::get(&db, &rec.id).unwrap();
        assert_eq!(r.transcript_status, JobStatus::Pending);
        assert_eq!(r.transcript_error, None);
        let attempts: i64 = db
            .tx(|t| {
                Ok(t.query_row(
                    "SELECT transcript_attempts FROM recordings WHERE id = ?1",
                    [&rec.id],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert_eq!(attempts, 0);
    }
}
