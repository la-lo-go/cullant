use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::db::{migrations, Db};
use crate::error::{AppError, AppResult};
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
fn spawn_scan(app: AppHandle, db: Arc<Db>, root: PathBuf) {
    std::thread::Builder::new()
        .name("scanner".into())
        .spawn(move || {
            if let Err(e) = scan::scan_project(&app, &db, &root) {
                tracing::error!("scan failed: {e}");
                let _ = app.emit("scan:error", e.to_string());
                return;
            }
            if let Err(e) = scan::metadata::run_metadata_pass(&app, &db, root.clone()) {
                tracing::error!("metadata pass failed: {e}");
            }
            match crate::thumbs::pregenerate_all(&db, &root, |done, total| {
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

/// Shared by the IPC command and the CULLANT_OPEN_PROJECT dev/startup hook.
pub fn do_open_project(path: &str, app: &AppHandle, state: &AppState) -> AppResult<ProjectInfo> {
    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(AppError::Other(format!("not a directory: {path}")));
    }

    let db = Arc::new(Db::open(&root)?);
    let db_path = db.path().to_string_lossy().into_owned();
    let root_str = root.to_string_lossy().into_owned();

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

    let thumbs = ThumbPool::start(db.clone(), root.clone());
    *state.project.lock().unwrap() = Some(ProjectState {
        root: root.clone(),
        db: db.clone(),
        thumbs,
    });
    tracing::info!("opened project at {root_str}");

    spawn_scan(app.clone(), db, root);

    Ok(ProjectInfo {
        root_path: root_str,
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
    let (db, root) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(AppError::NoProject)?;
        (project.db.clone(), project.root.clone())
    };
    spawn_scan(app, db, root);
    Ok(())
}

#[tauri::command]
pub fn close_project(state: State<'_, AppState>) {
    *state.project.lock().unwrap() = None;
}
