//! Stow desktop app: Tauri shell over stow-core.

mod app_nap;
mod capture_panel;
mod chrome;
mod commands;
mod error;
mod events;
mod pairing;
mod permissions;
mod scheduler;
mod server;
mod shortcut;
mod state;
#[cfg(test)]
mod testutil;
mod tray;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use stow_core::jobs::{JobEvent, JobHook, JobQueue};
use stow_core::repo::settings::{self, Key};
use stow_core::StowError;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tracing_subscriber::EnvFilter;

use events::{HotkeyStatus, Id};
use state::{lock, AppState};

/// `STOW_DATA_DIR` if set, otherwise the platform default.
fn data_dir() -> PathBuf {
    std::env::var_os("STOW_DATA_DIR")
        .map(PathBuf::from)
        .or_else(stow_core::default_data_dir)
        .unwrap_or_else(|| PathBuf::from("Stow"))
}

/// Translate core job events into window events the frontend listens for.
fn job_hook(app: tauri::AppHandle) -> JobHook {
    Arc::new(move |ev: JobEvent| {
        let r = match ev {
            JobEvent::BucketsChanged => app.emit(events::BUCKETS_CHANGED, events::Empty {}),
            JobEvent::ItemAdded(id) => app.emit(events::ITEM_ADDED, Id { id }),
            JobEvent::ItemUpdated(id) => app.emit(events::ITEM_UPDATED, Id { id }),
            JobEvent::RecordingUpdated(id) => app.emit(events::RECORDING_UPDATED, Id { id }),
            JobEvent::AiRunUpdated(id) => app.emit(events::AI_RUN_UPDATED, Id { id }),
            JobEvent::Progress {
                kind,
                received,
                total,
            } => app.emit(
                events::MODEL_DOWNLOAD_PROGRESS,
                events::Progress {
                    kind,
                    received,
                    total,
                },
            ),
        };
        if let Err(e) = r {
            tracing::warn!("emit failed: {e}");
        }
    })
}

/// Only the app's own pages may load in its webviews; links open in the browser via
/// the opener plugin instead.
fn navigation_allowed(url: &url::Url, dev: bool) -> bool {
    match url.scheme() {
        "tauri" => true,
        "about" => url.as_str() == "about:blank",
        "http" => {
            let host = url.host_str();
            host == Some("tauri.localhost")
                || (dev && host == Some("localhost") && url.port() == Some(1420))
        }
        _ => false,
    }
}

fn navigation_guard() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("navigation-guard")
        .on_navigation(|_webview, url| {
            let ok = navigation_allowed(url, cfg!(debug_assertions));
            if !ok {
                tracing::warn!(%url, "blocked navigation");
            }
            ok
        })
        .build()
}

/// What the startup dialog says when the library cannot be opened: a plain sentence,
/// the error, and where the data lives so the user can act on it.
pub fn startup_failure_message(err: &StowError, dir: &Path) -> String {
    let lead = match err {
        StowError::SchemaTooNew { .. } => {
            "This library was saved by a newer version of Stow. Update Stow to open it."
        }
        _ => "Stow couldn't open its library.",
    };
    format!("{lead}\n\n{err}\n\nData folder: {}", dir.display())
}

/// Managed when setup bailed out: keeps the event loop alive (no windows are left) until
/// the error dialog has been dismissed and `exit(1)` arrives.
struct StartupFailed;

/// Tear down the windows, show `message` in a native error dialog, then exit with
/// status 1. The dialog needs the event loop, which only starts once setup returns, and
/// `blocking_show` may not run on the main thread, so it waits on its own thread.
fn fail_startup(app: &tauri::AppHandle, message: String) {
    app.manage(StartupFailed);
    for window in app.webview_windows().values() {
        let _ = window.destroy();
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        handle
            .dialog()
            .message(&message)
            .title("Stow can't start")
            .kind(MessageDialogKind::Error)
            .blocking_show();
        handle.exit(1);
    });
}

/// Build and run the Tauri app (windows, tray, hotkey, extension server, scheduler).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
#[allow(clippy::too_many_lines)] // one Tauri builder chain plus its run loop
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let builder = tauri::Builder::default()
        // First, so a second copy (e.g. the LaunchAgent plus a manual launch) just focuses
        // the running one instead of double-firing reminders.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = commands::window::show_main_window(app);
        }))
        .plugin(navigation_guard())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ));

    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_global_shortcut::Builder::new().build());

    let app = builder
        .setup(|app| {
            let dir = data_dir();
            tracing::info!(data_dir = %dir.display(), "opening database");
            let db = match stow_core::Db::open(&dir) {
                Ok(db) => Arc::new(db),
                Err(e) => {
                    tracing::error!(data_dir = %dir.display(), "could not open the database: {e}");
                    fail_startup(app.handle(), startup_failure_message(&e, &dir));
                    return Ok(());
                }
            };
            // The asset protocol (snapshot images, audio playback) is scoped to the default
            // data dir in tauri.conf.json; a custom STOW_DATA_DIR needs these at runtime.
            for sub in ["assets", "audio"] {
                app.asset_protocol_scope()
                    .allow_directory(dir.join(sub), true)?;
            }
            let handle = app.handle().clone();
            let pairing = Arc::new(pairing::PairingState::default());

            let pair_handle = handle.clone();
            let on_pair: server::PairNotifier = Arc::new(move |req| {
                let _ = commands::window::show_main_window(&pair_handle);
                if let Err(e) = pair_handle.emit_to("main", events::PAIR_REQUEST, req) {
                    tracing::warn!("emit failed: {e}");
                }
            });
            let (jobs, server_info) = tauri::async_runtime::block_on(async {
                // Fetching private/loopback addresses is off unless the user opts in
                // (Settings → Advanced); captured local URLs are still saved.
                let allow_local_urls =
                    stow_core::repo::settings::get(&db, commands::settings::ALLOW_LOCAL_URLS_KEY)?
                        .is_some_and(|v| v == "true");
                let jobs = JobQueue::start_with(
                    db.clone(),
                    job_hook(handle.clone()),
                    stow_core::jobs::JobQueueOptions { allow_local_urls },
                )?;
                let server =
                    server::start(db.clone(), jobs.clone(), pairing.clone(), on_pair).await;
                Ok::<_, stow_core::StowError>((jobs, server))
            })?;
            match server_info {
                Ok(info) => tracing::info!(port = info.port, "extension endpoint listening"),
                Err(e) => tracing::warn!("extension endpoint failed to start: {e}"),
            }
            // Work that was in flight when the app last quit.
            match stow_core::jobs::resume_pending(&db, &jobs) {
                Ok(resumed) if !resumed.is_empty() => {
                    tracing::info!(count = resumed.len(), "resumed unfinished jobs");
                }
                Err(e) => tracing::warn!("could not resume jobs: {e}"),
                _ => {}
            }

            let hotkey = settings::get_key(&db, Key::Hotkey)?;
            let auto_stop_handle = handle.clone();
            let on_auto_stop: stow_core::audio::AutoStopHook = Arc::new(move || {
                commands::recordings::on_auto_stop(auto_stop_handle.clone());
            });
            app.manage(AppState {
                db: db.clone(),
                jobs,
                recorder: Arc::new(stow_core::audio::RecorderHandle::spawn_with_limit(
                    stow_core::audio::MAX_RECORDING,
                    Some(on_auto_stop),
                )?),
                recording_parent: Mutex::new(None),
                hotkey: Mutex::new(HotkeyStatus {
                    hotkey: hotkey.clone(),
                    registered: false,
                    error: None,
                }),
                pairing,
                tray: Mutex::new(None),
                tray_model: Mutex::new(tray::TrayModel::default()),
                quit_ready: AtomicBool::new(false),
                stow_was_active: AtomicBool::new(true),
                quit_on_close: AtomicBool::new(
                    settings::get_bool(&db, Key::QuitOnClose).unwrap_or(false),
                ),
                last_fired: Mutex::new(None),
            });

            // A hotkey another app owns is reported via `hotkey-status`, not fatal.
            let status = shortcut::register(&handle, &hotkey);
            shortcut::publish(&handle, status);
            if let Err(e) = tray::build(&handle, &hotkey) {
                tracing::warn!("tray failed: {e}");
            }
            #[cfg(target_os = "macos")]
            if let Some(capture) = app.get_webview_window("capture") {
                if let Err(e) = capture_panel::configure(&capture) {
                    tracing::warn!("capture panel setup failed: {e}");
                }
            }
            app_nap::disable_app_nap();
            scheduler::listen_for_clicks(&handle);
            scheduler::start(handle.clone(), db);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let quit = window.label() == "main"
                    && window
                        .try_state::<AppState>()
                        .is_some_and(|s| s.quit_on_close.load(Ordering::SeqCst));
                if quit {
                    api.prevent_close();
                    window.app_handle().exit(0);
                } else {
                    // Keep running in the menu bar so reminders still fire.
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::buckets::list_buckets,
            commands::buckets::create_bucket,
            commands::buckets::update_bucket,
            commands::buckets::delete_bucket,
            commands::buckets::reorder_buckets,
            commands::items::list_items,
            commands::items::get_item,
            commands::items::add_item,
            commands::items::move_item,
            commands::items::update_item_notes,
            commands::items::set_item_done,
            commands::items::delete_item,
            commands::items::refetch_item,
            commands::items::read_snapshot,
            commands::tasks::list_tasks,
            commands::tasks::get_task,
            commands::tasks::add_task,
            commands::tasks::toggle_task,
            commands::tasks::rename_task,
            commands::tasks::delete_task,
            commands::reminders::list_reminders,
            commands::reminders::add_reminder,
            commands::reminders::snooze_reminder,
            commands::reminders::delete_reminder,
            commands::reminders::reminder_agenda,
            commands::attachments::list_attachments,
            commands::attachments::add_attachment,
            commands::attachments::delete_attachment,
            commands::recordings::list_recordings,
            commands::recordings::is_recording,
            commands::recordings::start_recording,
            commands::recordings::stop_recording,
            commands::recordings::delete_recording,
            commands::recordings::retranscribe,
            commands::recordings::whisper_model_downloaded,
            commands::recordings::download_whisper_model,
            commands::ai::list_ai_runs,
            commands::ai::run_ai,
            commands::ai::set_api_key,
            commands::ai::has_api_key,
            commands::ai::delete_api_key,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::capture_settings,
            commands::settings::set_close_tab_after_capture,
            commands::search::search,
            commands::chrome::capture_front_tab,
            commands::chrome::close_front_tab,
            commands::chrome::list_chrome_tabs,
            commands::chrome::import_chrome_tabs,
            commands::window::hide_capture,
            commands::window::show_main,
            commands::window::ready_to_quit,
            commands::system::permission_status,
            commands::system::request_automation,
            commands::system::request_microphone,
            commands::system::open_privacy_settings,
            commands::system::pairing_code,
            commands::system::rotate_extension_token,
            commands::system::respond_pair,
            commands::system::export_library,
            commands::system::backup_database,
            commands::system::delete_whisper_models,
            commands::system::reset_all_data,
            commands::system::open_attachment,
            commands::system::reveal_data_dir,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    // Give the frontend a moment to flush pending note edits before the process exits
    // (⌘Q, tray Quit, or the dock menu all arrive here as ExitRequested). It calls
    // `ready_to_quit` when done; the 800 ms timer is the fallback.
    app.run(|app, event| match event {
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => {
            let recent = app.try_state::<AppState>().and_then(|s| {
                scheduler::take_recent(&mut lock(&s.last_fired), std::time::Instant::now())
            });
            let _ = match recent {
                Some(target) => commands::window::navigate(app, target),
                None => commands::window::show_main_window(app),
            };
        }
        // Setup failed and destroyed the windows; wait for the dialog's `exit(1)`.
        RunEvent::ExitRequested {
            api, code: None, ..
        } if app.try_state::<StartupFailed>().is_some() => api.prevent_exit(),
        RunEvent::ExitRequested { api, code, .. }
            if app
                .try_state::<AppState>()
                .is_some_and(|s| !s.quit_ready.swap(true, Ordering::SeqCst)) =>
        {
            api.prevent_exit();
            let _ = app.emit(events::APP_WILL_QUIT, events::Empty {});
            let handle = app.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(800));
                handle.exit(code.unwrap_or(0));
            });
        }
        RunEvent::Exit => {
            // Let running jobs finish briefly; anything cut off is put back to `pending`
            // so the next launch resumes it instead of marking it interrupted.
            if let Some(state) = app.try_state::<AppState>() {
                commands::recordings::save_on_exit(&state);
                let jobs = state.jobs.clone();
                if !tauri::async_runtime::block_on(jobs.shutdown()) {
                    tracing::info!("jobs still running at exit; they resume next launch");
                }
                match stow_core::jobs::requeue_running(&state.db) {
                    Ok(n) if n > 0 => tracing::info!(count = n, "requeued running jobs"),
                    Ok(_) => {}
                    Err(e) => tracing::warn!("could not requeue running jobs: {e}"),
                }
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_guard_allows_only_app_pages() {
        let ok = |s: &str, dev: bool| navigation_allowed(&url::Url::parse(s).unwrap(), dev);
        assert!(ok("tauri://localhost/index.html", false));
        assert!(ok("http://tauri.localhost/#/capture", false));
        assert!(ok("about:blank", false));
        assert!(ok("http://localhost:1420/", true));
        assert!(!ok("http://localhost:1420/", false));
        assert!(!ok("http://localhost:8080/", true));
        assert!(!ok("https://evil.example/", true));
        assert!(!ok("file:///etc/passwd", true));
        assert!(!ok("http://tauri.localhost.evil.example/", false));
    }

    #[test]
    fn startup_failure_message_is_friendly_and_names_the_data_dir() {
        let dir = Path::new("/Volumes/Data/Stow");
        let m = startup_failure_message(
            &StowError::SchemaTooNew {
                found: 9,
                supported: 2,
            },
            dir,
        );
        assert!(
            m.starts_with("This library was saved by a newer version of Stow."),
            "{m}"
        );
        assert!(m.contains("newer than this build supports"), "{m}");
        assert!(m.ends_with("Data folder: /Volumes/Data/Stow"), "{m}");
        let m = startup_failure_message(&StowError::Internal("disk full".into()), dir);
        assert!(m.starts_with("Stow couldn't open its library."), "{m}");
        assert!(
            m.contains("disk full") && m.contains("/Volumes/Data/Stow"),
            "{m}"
        );
    }
}
