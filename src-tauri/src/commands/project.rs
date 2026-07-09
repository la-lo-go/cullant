use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::db::{migrations, Db};
use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;
use crate::thumbs::ThumbPool;
use crate::{scan, AppState, ProjectState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub root_path: String,
    pub db_path: String,
    pub schema_version: i64,
    pub file_count: i64,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ThumbsProgress {
    done: usize,
    total: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ThumbsDone {
    total: usize,
}

/// Kick off scan + metadata pass + thumbnail pregeneration on a background
/// thread and notify the UI through scan:progress / scan:done /
/// metadata:done / thumbs:progress / thumbs:done events.
fn spawn_scan(app: AppHandle, db: Arc<Db>, store: Arc<dyn ProjectStore>, root: PathBuf) {
    std::thread::Builder::new()
        .name("scanner".into())
        .spawn(move || {
            if let Err(e) = scan::scan_project(&app, &db, store.as_ref()) {
                tracing::error!("scan failed: {e}");
                let _ = app.emit("scan:error", e.to_string());
                return;
            }
            if let Err(e) = scan::metadata::run_metadata_pass(&app, &db, store.as_ref()) {
                tracing::error!("metadata pass failed: {e}");
            }
            match crate::thumbs::pregenerate_all(&db, store.as_ref(), &root, |done, total| {
                let _ = app.emit("thumbs:progress", ThumbsProgress { done, total });
            }) {
                Ok(total) => {
                    let _ = app.emit("thumbs:done", ThumbsDone { total });
                }
                Err(e) => tracing::error!("thumb pregeneration failed: {e}"),
            }
        })
        .expect("failed to spawn scanner thread");
}

#[tauri::command]
pub fn open_project(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectInfo> {
    do_open_project(&path, &app, &state)
}

/// Launch the Android SAF folder picker and return the picked `content://` tree
/// URI (which `open_project` accepts as its `path`). `None` if the user
/// cancelled. On desktop this is a no-op returning `None` — the frontend uses
/// the native directory dialog there instead.
#[tauri::command]
pub fn pick_saf_tree(app: AppHandle) -> AppResult<Option<String>> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_saf::SafExt;
        match app.saf().open_tree() {
            Ok((tree_uri, _root)) => Ok(Some(tree_uri)),
            Err(e) => {
                tracing::info!("SAF picker returned no folder: {e}");
                Ok(None)
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(None)
    }
}

/// Shared by the IPC command and the CULLANT_OPEN_PROJECT dev/startup hook.
pub fn do_open_project(path: &str, app: &AppHandle, state: &AppState) -> AppResult<ProjectInfo> {
    // On Android a project is a SAF `content://` tree, not a filesystem path.
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        return open_saf_project(path, app, state);
    }

    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(AppError::Other(format!("not a directory: {path}")));
    }

    let db = Arc::new(Db::open(&root)?);
    let db_path = db.path().to_string_lossy().into_owned();
    let root_str = root.to_string_lossy().into_owned();
    let store: Arc<dyn ProjectStore> = Arc::new(crate::store::LocalFsStore::new(root.clone()));

    let root_for_db = root_str.clone();
    let (schema_version, file_count) = db.call(move |conn| {
        conn.execute(
            "INSERT INTO project (id, root_path, created_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
            params![root_for_db, unix_now()],
        )?;
        let version = migrations::current_version(conn)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |row| {
                row.get(0)
            })?;
        Ok((version, count))
    })?;

    let thumbs = ThumbPool::start(db.clone(), store.clone(), root.clone());
    *state.project.lock().unwrap() = Some(ProjectState {
        root: root.clone(),
        store: store.clone(),
        db: db.clone(),
        thumbs,
    });
    tracing::info!("opened project at {root_str}");
    crate::commands::recent::record_opened(app, &root_str);

    spawn_scan(app.clone(), db, store, root);

    Ok(ProjectInfo {
        root_path: root_str,
        db_path,
        schema_version,
        file_count,
    })
}

/// Real-filesystem base dir holding a project's `.cullant` sidecar (DB + thumb
/// cache), given its stored identifier: a filesystem path on desktop, a SAF
/// `content://` tree URI on Android (the latter maps to a private app dir keyed
/// by a hash of the URI, since SQLite cannot run inside a SAF tree).
pub(crate) fn project_data_base<R: tauri::Runtime>(
    app: &AppHandle<R>,
    id: &str,
) -> AppResult<PathBuf> {
    #[cfg(target_os = "android")]
    if id.starts_with("content://") {
        use tauri::Manager;
        let hash = xxhash_rust::xxh3::xxh3_64(id.as_bytes());
        return Ok(app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Other(format!("app_data_dir: {e}")))?
            .join("projects")
            .join(format!("{hash:016x}")));
    }
    let _ = app; // used only on Android
    Ok(PathBuf::from(id))
}

/// Open a SAF-backed project (Android). The picked `content://` tree hosts the
/// media; the DB and thumbnail cache live in private app storage keyed by a hash
/// of the tree URI, because SQLite cannot run inside a SAF tree.
#[cfg(target_os = "android")]
fn open_saf_project(tree_uri: &str, app: &AppHandle, state: &AppState) -> AppResult<ProjectInfo> {
    use tauri_plugin_saf::SafExt;

    let root_doc = app
        .saf()
        .root_document_id(tree_uri)
        .map_err(|e| AppError::Other(format!("saf: {e}")))?;

    let base = project_data_base(app, tree_uri)?;
    std::fs::create_dir_all(&base)?;

    let db = Arc::new(Db::open(&base)?);
    let db_path = db.path().to_string_lossy().into_owned();
    let store: Arc<dyn ProjectStore> = Arc::new(crate::store::SafStore::new(
        tree_uri.to_string(),
        root_doc,
        app.clone(),
        db.clone(),
    ));

    let tree_owned = tree_uri.to_string();
    let (schema_version, file_count) = db.call(move |conn| {
        conn.execute(
            "INSERT INTO project (id, root_path, created_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
            params![tree_owned, unix_now()],
        )?;
        let version = migrations::current_version(conn)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |row| {
                row.get(0)
            })?;
        Ok((version, count))
    })?;

    let thumbs = ThumbPool::start(db.clone(), store.clone(), base.clone());
    *state.project.lock().unwrap() = Some(ProjectState {
        root: base.clone(),
        store: store.clone(),
        db: db.clone(),
        thumbs,
    });
    tracing::info!("opened SAF project {tree_uri}");
    crate::commands::recent::record_opened(app, tree_uri);

    spawn_scan(app.clone(), db, store, base);

    Ok(ProjectInfo {
        root_path: tree_uri.to_string(),
        db_path,
        schema_version,
        file_count,
    })
}

/// Project already open in this session, if any (used by the frontend on
/// startup, e.g. after an auto-open via CULLANT_OPEN_PROJECT).
#[tauri::command]
pub fn current_project(state: State<'_, AppState>) -> AppResult<Option<ProjectInfo>> {
    let (db, root) = {
        let guard = state.project.lock().unwrap();
        match guard.as_ref() {
            None => return Ok(None),
            Some(p) => (p.db.clone(), p.root.clone()),
        }
    };
    let db_path = db.path().to_string_lossy().into_owned();
    let (schema_version, file_count) = db.call(|conn| {
        let version = migrations::current_version(conn)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |row| {
                row.get(0)
            })?;
        Ok((version, count))
    })?;
    Ok(Some(ProjectInfo {
        root_path: root.to_string_lossy().into_owned(),
        db_path,
        schema_version,
        file_count,
    }))
}

#[tauri::command]
pub fn rescan_project(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let (db, store, root) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(AppError::NoProject)?;
        (
            project.db.clone(),
            project.store.clone(),
            project.root.clone(),
        )
    };
    spawn_scan(app, db, store, root);
    Ok(())
}

#[tauri::command]
pub fn close_project(state: State<'_, AppState>) {
    *state.project.lock().unwrap() = None;
}
