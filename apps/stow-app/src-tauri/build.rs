//! Tauri build script. Every app command is declared here so it gets an
//! `allow-<command>` permission and is ACL-checked per window (see `capabilities/`).
//! Keep in sync with `generate_handler!` in src/lib.rs.

const COMMANDS: &[&str] = &[
    "add_attachment",
    "add_item",
    "add_reminder",
    "add_task",
    "app_info",
    "backup_database",
    "capture_front_tab",
    "capture_settings",
    "close_front_tab",
    "create_bucket",
    "delete_api_key",
    "delete_attachment",
    "delete_bucket",
    "delete_item",
    "delete_recording",
    "delete_reminder",
    "delete_task",
    "delete_whisper_models",
    "download_whisper_model",
    "export_library",
    "get_item",
    "get_settings",
    "get_task",
    "has_api_key",
    "hide_capture",
    "import_chrome_tabs",
    "is_recording",
    "list_ai_runs",
    "list_attachments",
    "list_buckets",
    "list_chrome_tabs",
    "list_items",
    "list_recordings",
    "list_reminders",
    "list_tasks",
    "move_item",
    "open_attachment",
    "open_privacy_settings",
    "pairing_code",
    "permission_status",
    "read_snapshot",
    "ready_to_quit",
    "refetch_item",
    "reminder_agenda",
    "rename_task",
    "reorder_buckets",
    "request_automation",
    "request_microphone",
    "reset_all_data",
    "respond_pair",
    "reveal_data_dir",
    "retranscribe",
    "rotate_extension_token",
    "run_ai",
    "search",
    "set_api_key",
    "set_close_tab_after_capture",
    "set_item_done",
    "show_main",
    "snooze_reminder",
    "start_recording",
    "stop_recording",
    "toggle_task",
    "update_bucket",
    "update_item_notes",
    "update_settings",
    "whisper_model_downloaded",
];

/// whisper.cpp's Metal backend uses `@available` checks, which compile to
/// `___isPlatformVersionAtLeast` from compiler-rt. Rust links with `-nodefaultlibs`,
/// so in release (where the call survives) we add clang's builtins archive ourselves.
fn link_compiler_rt() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let out = std::process::Command::new("cc")
        .arg("-print-resource-dir")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    if let Some(dir) = out {
        let lib = std::path::Path::new(&dir).join("lib").join("darwin");
        if lib.join("libclang_rt.osx.a").exists() {
            println!("cargo:rustc-link-search=native={}", lib.display());
            println!("cargo:rustc-link-lib=static=clang_rt.osx");
        } else {
            println!(
                "cargo:warning=libclang_rt.osx.a not found in {}",
                lib.display()
            );
        }
    }
}

/// Build metadata for `app_info().build`: the build time, and the commit when CI sets
/// `STOW_COMMIT` (local builds leave it unset, so `commit` is null there).
fn emit_build_info() {
    println!("cargo:rerun-if-env-changed=STOW_COMMIT");
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    println!("cargo:rustc-env=STOW_BUILT_AT_UNIX={secs}");
}

fn main() {
    link_compiler_rt();
    emit_build_info();
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
