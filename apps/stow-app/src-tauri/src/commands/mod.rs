//! Tauri commands. Names and argument shapes are the contract in
//! docs/superpowers/plans/tauri-contract.md. Anything touching the database, files,
//! the Keychain, `AppleScript` or the recorder runs off the main thread.

pub mod ai;
pub mod attachments;
pub mod buckets;
pub mod chrome;
pub mod items;
pub mod recordings;
pub mod reminders;
pub mod search;
pub mod settings;
pub mod system;
pub mod tasks;
pub mod window;

use std::path::PathBuf;

use serde::Serialize;
use stow_core::Db;
use tauri::{AppHandle, Manager, State};

use crate::error::CmdResult;
use crate::state::AppState;

/// Run database work on the blocking pool.
pub async fn db<T: Send + 'static>(
    state: &AppState,
    f: impl FnOnce(&Db) -> stow_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    Ok(state.db.call(f).await?)
}

#[derive(Serialize)]
pub struct AppInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub data_dir: String,
    /// The bundled unpacked Chrome extension (for "Load unpacked"), when present.
    pub extension_dir: Option<String>,
    pub build: BuildInfo,
}

/// How this binary was built (Settings → About).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BuildInfo {
    pub version: &'static str,
    /// `STOW_COMMIT` at build time (CI sets it); null for local builds.
    pub commit: Option<&'static str>,
    /// RFC 3339 UTC build time.
    pub built_at: String,
}

/// The unix build time that build.rs records, as RFC 3339.
pub fn built_at(unix_secs: i64) -> String {
    chrono::DateTime::from_timestamp(unix_secs, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

pub fn build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        commit: option_env!("STOW_COMMIT").filter(|c| !c.trim().is_empty()),
        built_at: built_at(env!("STOW_BUILT_AT_UNIX").parse().unwrap_or(0)),
    }
}

/// Where the unpacked extension may be, most preferred first: dev builds use the repo's
/// `extension/` (so edits there are what "Load unpacked" picks up) before the copy
/// tauri-build puts next to the binary; release builds only `Resources/extension`
/// (see `bundle.resources`).
pub fn extension_candidates(resource_dir: Option<PathBuf>, dev: bool) -> Vec<PathBuf> {
    let repo = dev.then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../extension"));
    repo.into_iter()
        .chain(resource_dir.map(|d| d.join("extension")))
        .collect()
}

fn extension_dir(app: &AppHandle) -> Option<PathBuf> {
    extension_candidates(app.path().resource_dir().ok(), cfg!(debug_assertions))
        .into_iter()
        .find(|p| p.join("manifest.json").is_file())
        .map(|p| p.canonicalize().unwrap_or(p))
}

#[tauri::command]
#[allow(clippy::unnecessary_wraps)] // every command returns CmdResult for a uniform IPC shape
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> CmdResult<AppInfo> {
    Ok(AppInfo {
        name: "Stow",
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.db.data_dir().to_string_lossy().into_owned(),
        extension_dir: extension_dir(&app).map(|p| p.to_string_lossy().into_owned()),
        build: build_info(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_dir_prefers_the_repo_in_dev_and_only_resources_in_release() {
        let res = PathBuf::from("/Applications/Stow.app/Contents/Resources");
        let dev = extension_candidates(Some(res.clone()), true);
        assert_eq!(dev.len(), 2);
        assert!(
            dev[0].join("manifest.json").is_file(),
            "{}",
            dev[0].display()
        );
        assert!(dev[0].ends_with("extension"));
        assert_eq!(dev[1], res.join("extension"));
        assert_eq!(
            extension_candidates(Some(res.clone()), false),
            vec![res.join("extension")]
        );
        assert!(extension_candidates(None, false).is_empty());
    }

    #[test]
    fn build_info_has_version_and_rfc3339_time() {
        let b = build_info();
        assert_eq!(b.version, env!("CARGO_PKG_VERSION"));
        assert!(
            chrono::DateTime::parse_from_rfc3339(&b.built_at).is_ok(),
            "{}",
            b.built_at
        );
        assert_eq!(built_at(0), "1970-01-01T00:00:00Z");
        assert!(b.commit.is_none_or(|c| !c.is_empty()));
        let v = serde_json::to_value(&b).unwrap();
        assert!(
            v.get("commit").is_some(),
            "commit is present even when null: {v}"
        );
    }
}
