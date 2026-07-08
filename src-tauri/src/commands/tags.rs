use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::engine::culling::Targets;
use crate::engine::tags::{self, TagChange, TaskTag};
use crate::error::{AppError, AppResult};
use crate::AppState;

fn project_db(state: &AppState) -> AppResult<Arc<Db>> {
    let guard = state.project.lock().unwrap();
    Ok(guard.as_ref().ok_or(AppError::NoProject)?.db.clone())
}

#[tauri::command]
pub fn list_task_tags(state: State<'_, AppState>) -> AppResult<Vec<TaskTag>> {
    tags::list(&project_db(&state)?)
}

#[tauri::command]
pub fn create_task_tag(
    name: String,
    shortcut: Option<String>,
    scope: i64,
    color: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<TaskTag> {
    let tag = tags::create(&project_db(&state)?, name, shortcut, scope, color)?;
    let _ = app.emit("tags:changed", ());
    Ok(tag)
}

#[tauri::command]
pub fn update_task_tag(tag: TaskTag, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    tags::update(&project_db(&state)?, tag)?;
    let _ = app.emit("tags:changed", ());
    Ok(())
}

#[tauri::command]
pub fn delete_task_tag(tag_id: i64, app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    tags::delete(&project_db(&state)?, tag_id)?;
    let _ = app.emit("tags:changed", ());
    Ok(())
}

#[tauri::command]
pub fn toggle_task_tag(
    targets: Targets,
    tag_id: i64,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Vec<TagChange>> {
    let changes = tags::toggle(&project_db(&state)?, targets, tag_id)?;
    let _ = app.emit("filetags:changed", &changes);
    Ok(changes)
}
