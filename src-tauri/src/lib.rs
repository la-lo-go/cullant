mod commands;
mod db;
mod decode;
mod engine;
mod error;
mod protocol;
mod scan;
mod store;
mod thumbs;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use db::Db;
use store::ProjectStore;
use thumbs::ThumbPool;

/// State of the currently open project.
pub struct ProjectState {
    /// Real-filesystem base for this project's sidecar data (the `.cullant`
    /// dir): the project root itself on desktop, a private app dir on Android.
    pub root: PathBuf,
    /// Backend for the project's media (real fs on desktop, SAF on Android).
    pub store: Arc<dyn ProjectStore>,
    pub db: Arc<Db>,
    pub thumbs: ThumbPool,
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
        .plugin(tauri_plugin_saf::init())
        .manage(AppState::default())
        .register_asynchronous_uri_scheme_protocol("cullant", |ctx, request, responder| {
            protocol::handle(ctx.app_handle(), request, responder);
        })
        .setup(|app| {
            // Dev/test hook: auto-open a project folder at startup.
            if let Ok(path) = std::env::var("CULLANT_OPEN_PROJECT") {
                use tauri::Manager;
                let state = app.state::<AppState>();
                match commands::project::do_open_project(&path, app.handle(), &state) {
                    Ok(info) => tracing::info!("auto-opened project {}", info.root_path),
                    Err(e) => tracing::error!("CULLANT_OPEN_PROJECT failed: {e}"),
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::project::open_project,
            commands::project::pick_saf_tree,
            commands::project::current_project,
            commands::project::rescan_project,
            commands::project::close_project,
            commands::recent::list_recent_projects,
            commands::recent::remove_recent_project,
            commands::catalog::query_items,
            commands::catalog::media_counts,
            commands::culling::set_rating,
            commands::culling::set_flag,
            commands::culling::set_label,
            commands::groups::decouple_group,
            commands::groups::recouple_group,
            commands::tags::list_task_tags,
            commands::tags::create_task_tag,
            commands::tags::update_task_tag,
            commands::tags::delete_task_tag,
            commands::tags::toggle_task_tag,
            commands::actions::enqueue_action,
            commands::actions::remove_pending,
            commands::actions::clear_pending,
            commands::actions::list_pending,
            commands::actions::commit_preview,
            commands::actions::commit_execute,
            commands::actions::get_project_setting,
            commands::actions::set_project_setting,
            commands::metadata::get_file_metadata,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
