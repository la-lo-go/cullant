use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::engine::culling::{self, CullState, Targets};
use crate::error::{AppError, AppResult};
use crate::AppState;

fn project_db(state: &AppState) -> AppResult<Arc<Db>> {
    let guard = state.project.lock().unwrap();
    Ok(guard.as_ref().ok_or(AppError::NoProject)?.db.clone())
}

fn emit_changed(app: &AppHandle, db: &Db, changed: &[CullState]) {
    let _ = app.emit(
        "state:changed",
        serde_json::json!({"projectRoot": db.project_root(), "states": changed}),
    );
}

#[tauri::command]
pub fn set_rating(
    targets: Targets,
    rating: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<CullState>> {
    let db = project_db(&state)?;
    let changed = culling::set_rating(&db, targets, rating)?;
    emit_changed(&app, &db, &changed);
    Ok(changed)
}

#[tauri::command]
pub fn set_flag(
    targets: Targets,
    flag: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<CullState>> {
    let db = project_db(&state)?;
    let changed = culling::set_flag(&db, targets, flag)?;
    emit_changed(&app, &db, &changed);
    Ok(changed)
}

/// Turn the targets a quarter turn at a time; `steps` is positive clockwise.
#[tauri::command]
pub fn rotate(
    targets: Targets,
    steps: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<CullState>> {
    let db = project_db(&state)?;
    let changed = culling::rotate(&db, targets, steps)?;
    emit_changed(&app, &db, &changed);
    Ok(changed)
}

#[tauri::command]
pub fn set_label(
    targets: Targets,
    label: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<CullState>> {
    let db = project_db(&state)?;
    let changed = culling::set_label(&db, targets, label)?;
    emit_changed(&app, &db, &changed);
    Ok(changed)
}
