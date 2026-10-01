use tauri::{AppHandle, Emitter, State};

use crate::engine::culling::CullState;
use crate::engine::groups::{self, SyncFrom};
use crate::error::{AppError, AppResult};
use crate::AppState;

#[tauri::command]
pub fn decouple_group(group_id: i64, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let db = {
        let guard = state.project.lock().unwrap();
        guard.as_ref().ok_or(AppError::NoProject)?.db.clone()
    };
    groups::decouple(&db, group_id)?;
    let _ = app.emit(
        "groups:changed",
        serde_json::json!({"projectRoot": db.project_root(), "groupId": group_id}),
    );
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
    let _ = app.emit(
        "groups:changed",
        serde_json::json!({"projectRoot": db.project_root(), "groupId": group_id}),
    );
    let _ = app.emit(
        "pending:changed",
        serde_json::json!({"projectRoot": db.project_root()}),
    );
    Ok(())
}

/// Make a coupled pair agree again. Emits `state:changed` rather than
/// `groups:changed`: the grouping is untouched, only the culling state moves.
#[tauri::command]
pub fn sync_pair_state(
    group_id: i64,
    sync_from: SyncFrom,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<CullState>> {
    let db = {
        let guard = state.project.lock().unwrap();
        guard.as_ref().ok_or(AppError::NoProject)?.db.clone()
    };
    let states = groups::sync_state(&db, group_id, sync_from)?;
    let _ = app.emit(
        "state:changed",
        serde_json::json!({"projectRoot": db.project_root(), "states": &states}),
    );
    let _ = app.emit(
        "pending:changed",
        serde_json::json!({"projectRoot": db.project_root()}),
    );
    Ok(states)
}
