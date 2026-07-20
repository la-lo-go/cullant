use std::sync::Arc;

use rusqlite::params;
use tauri::{AppHandle, State};

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;
use crate::AppState;

fn project(state: &AppState) -> AppResult<(Arc<Db>, Arc<dyn ProjectStore>)> {
    let guard = state.project.lock().unwrap();
    let p = guard.as_ref().ok_or(AppError::NoProject)?;
    Ok((p.db.clone(), p.store.clone()))
}

/// Open a media file in the OS default external application. The escape hatch
/// for videos the in-app WebView can't decode (mainly on Android, where codec
/// support is narrower). Local-filesystem projects go through the `opener`
/// plugin with the real path; SAF projects hand the `content://` document to an
/// Android chooser via the SAF store.
#[tauri::command]
pub fn open_external(file_id: i64, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let (db, store) = project(&state)?;

    let rel_path: String = db.call(move |conn| {
        Ok(conn.query_row(
            "SELECT rel_path FROM files WHERE id = ?1 AND status = 0",
            params![file_id],
            |r| r.get(0),
        )?)
    })?;

    match store.local_path(&rel_path) {
        Some(path) => {
            use tauri_plugin_opener::OpenerExt;
            app.opener()
                .open_path(path.to_string_lossy().to_string(), None::<&str>)
                .map_err(|e| AppError::Other(format!("opener: {e}")))
        }
        None => store.open_external(&rel_path),
    }
}
