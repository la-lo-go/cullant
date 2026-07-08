use tauri::{AppHandle, Emitter, State};

use crate::engine::groups::{self, SyncFrom};
use crate::error::{AppError, AppResult};
use crate::AppState;

#[tauri::command]
pub fn decouple_group(
    group_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let db = {
        let guard = state.project.lock().unwrap();
        guard.as_ref().ok_or(AppError::NoProject)?.db.clone()
    };
    groups::decouple(&db, group_id)?;
    let _ = app.emit("groups:changed", group_id);
    Ok(())
}

#[tauri::command]
pub fn recouple_group(
    group_id: i64,
    sync_from: SyncFrom,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let db = {
        let guard = state.project.lock().unwrap();
        guard.as_ref().ok_or(AppError::NoProject)?.db.clone()
    };
    groups::recouple(&db, group_id, sync_from)?;
    let _ = app.emit("groups:changed", group_id);
    Ok(())
}
