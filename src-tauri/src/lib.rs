mod commands;
mod db;
mod error;
mod protocol;

use std::path::PathBuf;
use std::sync::Mutex;

use db::Db;

/// State of the currently open project.
pub struct ProjectState {
    pub root: PathBuf,
    pub db: Db,
}

#[derive(Default)]
pub struct AppState {
    pub project: Mutex<Option<ProjectState>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .register_asynchronous_uri_scheme_protocol("cullant", |_ctx, request, responder| {
            responder.respond(protocol::handle(request));
        })
        .invoke_handler(tauri::generate_handler![
            commands::project::open_project,
            commands::project::close_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
