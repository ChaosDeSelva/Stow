//! Permissions, extension pairing, export, backup and the Settings "danger zone".

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use serde::Serialize;
use stow_core::export::{self, ExportFormat, ExportReport};
use stow_core::repo::attachments;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use super::db;
use crate::error::{blocking, require_id, CmdError, CmdResult};
use crate::permissions::{self, Automation, Microphone, PermissionStatus};
use crate::server;
use crate::state::AppState;

/// Current Automation (Chrome) and microphone status, without prompting.
#[tauri::command]
pub async fn permission_status() -> CmdResult<PermissionStatus> {
    blocking(|| {
        Ok::<_, CmdError>(PermissionStatus {
            automation: permissions::automation(false),
            microphone: permissions::microphone(),
        })
    })
    .await
}

/// Prompt for Automation consent if undetermined; blocks until the user answers.
#[tauri::command]
pub async fn request_automation() -> CmdResult<Automation> {
    blocking(|| Ok::<_, CmdError>(permissions::automation(true))).await
}

/// Prompt for microphone access if undetermined (`AVCaptureDevice requestAccess`).
#[tauri::command]
pub async fn request_microphone() -> CmdResult<Microphone> {
    blocking(|| Ok::<_, CmdError>(permissions::request_microphone())).await
}

#[tauri::command]
pub async fn open_privacy_settings(pane: String) -> CmdResult<()> {
    let url =
        permissions::privacy_settings_url(&pane).ok_or_else(|| CmdError::invalid("pane", &pane))?;
    blocking(move || {
        let status = std::process::Command::new("/usr/bin/open")
            .arg(url)
            .status()
            .map_err(|e| CmdError::new("io", format!("could not run open: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(CmdError::new("io", format!("open exited with {status}")))
        }
    })
    .await
}

/// `"<port>:<token>"` for the extension's options page.
#[tauri::command]
pub async fn pairing_code(state: State<'_, AppState>) -> CmdResult<String> {
    db(&state, server::pairing_code).await
}

/// New token (the old one stops working immediately); returns the new pairing code.
#[tauri::command]
pub async fn rotate_extension_token(state: State<'_, AppState>) -> CmdResult<String> {
    db(&state, |d| {
        server::rotate_token(d)?;
        server::pairing_code(d)
    })
    .await
}

/// Answer a `pair-request`.
#[tauri::command]
pub fn respond_pair(state: State<'_, AppState>, id: String, allow: bool) -> CmdResult<()> {
    require_id("id", &id)?;
    if state.pairing.respond(&id, allow) {
        Ok(())
    } else {
        Err(CmdError::new(
            "not_found",
            format!("pairing request not found or expired: {id}"),
        ))
    }
}

#[derive(Debug, Serialize)]
pub struct Backup {
    pub path: String,
    pub bytes: u64,
}

/// An absolute destination path from a save/open dialog.
fn dest(field: &'static str, p: &str) -> CmdResult<PathBuf> {
    let path = PathBuf::from(p.trim());
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(CmdError::invalid(field, p))
    }
}

/// `format`: `markdown` | `json`.
pub fn parse_format(format: &str) -> CmdResult<ExportFormat> {
    match format.trim() {
        "markdown" => Ok(ExportFormat::Markdown),
        "json" => Ok(ExportFormat::Json),
        other => Err(CmdError::invalid("format", other)),
    }
}

#[tauri::command]
pub async fn export_library(
    state: State<'_, AppState>,
    format: String,
    dest_dir: String,
) -> CmdResult<ExportReport> {
    let format = parse_format(&format)?;
    let dir = dest("dest_dir", &dest_dir)?;
    db(&state, move |d| export::export_library(d, format, &dir)).await
}

#[tauri::command]
pub async fn backup_database(state: State<'_, AppState>, dest_path: String) -> CmdResult<Backup> {
    let path = dest("dest_path", &dest_path)?;
    let out = path.clone();
    let bytes = db(&state, move |d| export::backup_database(d, &out)).await?;
    Ok(Backup {
        path: path.to_string_lossy().into_owned(),
        bytes,
    })
}

#[derive(Debug, Serialize)]
pub struct Deleted {
    pub deleted_bytes: u64,
}

/// Remove downloaded Whisper models (`*.bin`) and interrupted downloads (`*.part`) from
/// `models_dir`; returns the bytes freed. A missing directory counts as empty.
pub fn delete_models_in(models_dir: &Path) -> std::io::Result<u64> {
    let entries = match std::fs::read_dir(models_dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut freed = 0;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let is_model = path
            .extension()
            .is_some_and(|ext| ext == "bin" || ext == "part");
        if is_model && entry.file_type()?.is_file() {
            freed += entry.metadata()?.len();
            std::fs::remove_file(&path)?;
        }
    }
    Ok(freed)
}

/// Stow's own files in a data dir: the database (with its WAL and shared-memory
/// sidecars) and the folders `Db::open` creates.
pub fn stow_files() -> impl Iterator<Item = &'static str> {
    ["stow.db", "stow.db-wal", "stow.db-shm"]
        .into_iter()
        .chain(stow_core::db::SUBDIRS.iter().copied())
}

/// Remove Stow's own files from `dir` (see [`stow_files`]), keeping `dir` itself and
/// anything else in it: a custom `STOW_DATA_DIR` may be shared with other things.
/// Returns the names left behind, which the caller logs. Missing files are fine.
pub fn clear_stow_files(dir: &Path) -> std::io::Result<Vec<String>> {
    for name in stow_files() {
        let path = dir.join(name);
        let removed = match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path),
            Ok(_) => std::fs::remove_file(&path),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => Err(e),
        };
        removed?;
    }
    let mut left: Vec<String> = std::fs::read_dir(dir)?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()?;
    left.sort();
    Ok(left)
}

/// True when `path` is inside `root` (both resolved through symlinks), so a stored
/// path that was tampered with cannot open something else.
pub fn path_within(root: &Path, path: &Path) -> bool {
    match (root.canonicalize(), path.canonicalize()) {
        (Ok(root), Ok(path)) => path.starts_with(root),
        _ => false,
    }
}

/// `reset_all_data` only proceeds when the user typed exactly `delete`.
pub fn check_reset_confirmation(confirm: &str) -> CmdResult<()> {
    if confirm == "delete" {
        Ok(())
    } else {
        Err(CmdError::invalid("confirm", confirm))
    }
}

fn io_error(what: &str, e: std::io::Error) -> CmdError {
    CmdError::new("io", format!("could not {what}: {e}"))
}

/// Delete the downloaded Whisper models; the next transcription re-downloads.
#[tauri::command]
pub async fn delete_whisper_models(state: State<'_, AppState>) -> CmdResult<Deleted> {
    let dir = state.db.data_dir().join("models");
    let deleted_bytes = blocking(move || {
        delete_models_in(&dir).map_err(|e| io_error("delete the Whisper models", e))
    })
    .await?;
    Ok(Deleted { deleted_bytes })
}

/// Wipe Stow's data and restart. Running jobs get the usual shutdown grace, then the
/// database and Stow's folders go (the dir itself and anything else in it stay, see
/// [`clear_stow_files`]); the restarted app recreates an empty library. The open
/// database handles keep working on the unlinked files until the process exits, so
/// nothing is written back.
#[tauri::command]
pub async fn reset_all_data(
    app: AppHandle,
    state: State<'_, AppState>,
    confirm: String,
) -> CmdResult<()> {
    check_reset_confirmation(&confirm)?;
    state.jobs.shutdown().await;
    let dir = state.db.data_dir().to_path_buf();
    let log_dir = dir.clone();
    let left =
        blocking(move || clear_stow_files(&dir).map_err(|e| io_error("clear the data folder", e)))
            .await?;
    if !left.is_empty() {
        tracing::warn!(
            dir = %log_dir.display(),
            entries = ?left,
            "reset left entries that are not Stow's in the data folder"
        );
    }
    crate::tray::refresh(&app);
    // The data is gone, so skip the autosave flush the exit handler would wait for.
    state.quit_ready.store(true, Ordering::SeqCst);
    app.request_restart();
    Ok(())
}

/// Open an attachment in its default app. Goes through Rust (not the opener plugin's
/// `openPath`) because the plugin's path scope is fixed at build time to the default
/// data dir and cannot follow a custom `STOW_DATA_DIR`.
#[tauri::command]
pub async fn open_attachment(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> CmdResult<()> {
    require_id("id", &id)?;
    let att = db(&state, move |d| attachments::get(d, &id)).await?;
    let root = state.db.data_dir().join("attachments");
    let path = PathBuf::from(att.path);
    blocking(move || {
        if !path_within(&root, &path) {
            return Err(CmdError::new(
                "io",
                format!("attachment is outside the data folder: {}", path.display()),
            ));
        }
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)
            .map_err(|e| CmdError::new("io", format!("could not open the attachment: {e}")))
    })
    .await
}

/// Show the data folder in Finder (same reason as [`open_attachment`]).
#[tauri::command]
pub async fn reveal_data_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.db.data_dir().to_path_buf();
    blocking(move || {
        app.opener()
            .open_path(dir.to_string_lossy(), None::<&str>)
            .map_err(|e| CmdError::new("io", format!("could not open the data folder: {e}")))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_models_removes_bin_and_part_only() {
        let dir = crate::testutil::TempDir::new();
        let models = dir.path().join("models");
        std::fs::create_dir(&models).unwrap();
        std::fs::write(models.join("ggml-base.en.bin"), [0u8; 10]).unwrap();
        std::fs::write(models.join("ggml-small.en.bin.part"), [0u8; 5]).unwrap();
        std::fs::write(models.join("notes.txt"), b"keep").unwrap();
        std::fs::create_dir(models.join("sub.bin")).unwrap();
        assert_eq!(delete_models_in(&models).unwrap(), 15);
        let left: Vec<_> = std::fs::read_dir(&models)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(left.len(), 2, "{left:?}");
        assert!(left.contains(&"notes.txt".to_string()) && left.contains(&"sub.bin".to_string()));
        assert_eq!(delete_models_in(&dir.path().join("missing")).unwrap(), 0);
    }

    #[test]
    fn clear_stow_files_deletes_only_stows_own_files() {
        let dir = crate::testutil::TempDir::new();
        // What `Db::open` creates, plus the WAL sidecars, plus a nested file.
        stow_core::Db::open(dir.path()).unwrap();
        for name in ["stow.db-wal", "stow.db-shm"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir_all(dir.path().join("audio/deep")).unwrap();
        std::fs::write(dir.path().join("audio/deep/a.wav"), b"x").unwrap();
        // Not Stow's: a custom STOW_DATA_DIR may hold other things.
        std::fs::write(dir.path().join("keep.txt"), b"mine").unwrap();
        std::fs::create_dir(dir.path().join("other-app")).unwrap();
        std::fs::write(dir.path().join("other-app/x"), b"x").unwrap();

        let left = clear_stow_files(dir.path()).unwrap();
        assert_eq!(left, vec!["keep.txt".to_string(), "other-app".to_string()]);
        assert!(dir.path().is_dir());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("keep.txt")).unwrap(),
            "mine"
        );
        assert!(dir.path().join("other-app/x").is_file());
        for name in stow_files() {
            assert!(!dir.path().join(name).exists(), "{name} removed");
        }
        // Idempotent: nothing of Stow's left is fine.
        assert_eq!(clear_stow_files(dir.path()).unwrap().len(), 2);
    }

    #[test]
    fn path_within_resolves_symlinks_and_rejects_escapes() {
        let dir = crate::testutil::TempDir::new();
        let root = dir.path().join("attachments");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(root.join("sub/a.pdf"), b"x").unwrap();
        std::fs::write(dir.path().join("outside.txt"), b"x").unwrap();
        assert!(path_within(&root, &root.join("sub/a.pdf")));
        assert!(!path_within(&root, &dir.path().join("outside.txt")));
        assert!(
            !path_within(&root, &root.join("../outside.txt")),
            "dot-dot escape"
        );
        assert!(!path_within(&root, &root.join("missing.pdf")), "must exist");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("outside.txt"), root.join("link")).unwrap();
            assert!(!path_within(&root, &root.join("link")), "symlink escape");
        }
    }

    #[test]
    fn reset_requires_the_exact_word() {
        assert!(check_reset_confirmation("delete").is_ok());
        for bad in ["", "Delete", "DELETE", " delete", "yes"] {
            assert_eq!(
                check_reset_confirmation(bad).unwrap_err().code,
                "invalid_input",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn export_args_are_validated() {
        assert_eq!(parse_format("markdown").unwrap(), ExportFormat::Markdown);
        assert_eq!(parse_format("json").unwrap(), ExportFormat::Json);
        assert_eq!(parse_format("pdf").unwrap_err().code, "invalid_input");
        assert_eq!(
            dest("dest_dir", "relative/dir").unwrap_err().code,
            "invalid_input"
        );
        assert!(dest("dest_dir", "/tmp/out").is_ok());
    }
}
