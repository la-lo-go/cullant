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

/// Thin facade for the `bench_ingest` example. Not a stable API.
#[doc(hidden)]
pub mod bench {
    use std::path::Path;
    use std::sync::Arc;

    pub use crate::db::Db;
    pub use crate::scan::ingest::PreviewMode;
    pub use crate::store::ProjectStore;

    pub fn open_db(root: &Path) -> Db {
        Db::open(root).expect("failed to open project db")
    }

    pub fn local_store(root: &Path) -> Arc<dyn ProjectStore> {
        Arc::new(crate::store::LocalFsStore::new(root))
    }

    /// Walk + reconcile; returns the number of present files.
    pub fn scan(db: &Arc<Db>, store: &dyn ProjectStore) -> i64 {
        crate::scan::scan_with_store(db, store, &mut |_| {})
            .expect("scan failed")
            .file_count
    }

    /// Run the fused ingest; returns (tier-1 total, background previews done).
    pub fn ingest(
        db: &Arc<Db>,
        store: &dyn ProjectStore,
        root: &Path,
        mode: PreviewMode,
    ) -> (usize, usize) {
        let mut tier1 = 0usize;
        let mut previews = 0usize;
        crate::scan::ingest::run_ingest_inner(
            db,
            store,
            root,
            mode,
            &mut |_, total| tier1 = total,
            &mut |_, _| {},
            &mut |done, _| previews = done,
        )
        .expect("ingest failed");
        (tier1, previews)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().init();

    // Mobile: cap the decode pool. Phones report 8 cores but sustain far
    // fewer under thermal/memory pressure, and each in-flight decode holds
    // multi-MB buffers. Best-effort — a later init error just keeps defaults.
    #[cfg(target_os = "android")]
    {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4))
            .unwrap_or(2);
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global();
    }

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
                match commands::project::do_open_project(
                    &path,
                    app.handle(),
                    &state,
                    Default::default(),
                ) {
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
