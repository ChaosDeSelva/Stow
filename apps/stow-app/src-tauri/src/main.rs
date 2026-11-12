// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
//! Stow desktop app binary; all logic is in `stow_app_lib`.

fn main() {
    stow_app_lib::run();
}
