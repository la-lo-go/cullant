use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::engine::actions::{self, ActionKind, PairScope, PendingAction};
use crate::engine::committer::{self, CommitOutcome, CommitPlan};
use crate::engine::culling::Targets;
use crate::error::{AppError, AppResult};
use crate::AppState;

fn project(state: &AppState) -> AppResult<(Arc<Db>, std::path::PathBuf)> {
    let guard = state.project.lock().unwrap();
    let p = guard.as_ref().ok_or(AppError::NoProject)?;
    Ok((p.db.clone(), p.root.clone()))
}

#[tauri::command]
pub fn enqueue_action(
    targets: Targets,
    action: ActionKind,
    dest: Option<String>,
    pair_scope: Option<PairScope>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    let (db, _) = project(&state)?;
    let n = actions::enqueue(&db, targets, action, dest, pair_scope.unwrap_or_default())?;
    let _ = app.emit("pending:changed", ());
    Ok(n)
}

#[tauri::command]
pub fn remove_pending(
    pending_ids: Vec<i64>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (db, _) = project(&state)?;
    actions::remove(&db, pending_ids)?;
    let _ = app.emit("pending:changed", ());
    Ok(())
}

#[tauri::command]
pub fn clear_pending(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let (db, _) = project(&state)?;
    actions::clear_all(&db)?;
    let _ = app.emit("pending:changed", ());
    Ok(())
}

#[tauri::command]
pub fn list_pending(state: State<'_, AppState>) -> AppResult<Vec<PendingAction>> {
    let (db, _) = project(&state)?;
    actions::list(&db)
}

#[tauri::command]
pub fn commit_preview(state: State<'_, AppState>) -> AppResult<CommitPlan> {
    let (db, root) = project(&state)?;
    committer::preview(&db, &root)
}

#[tauri::command]
pub fn commit_execute(
    plan_hash: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CommitOutcome> {
    let (db, root) = project(&state)?;
    let progress_app = app.clone();
    let outcome = committer::execute(&db, &root, &plan_hash, move |done, total| {
        let _ = progress_app.emit(
            "commit:progress",
            serde_json::json!({ "done": done, "total": total }),
        );
    })?;
    let _ = app.emit("pending:changed", ());
    let _ = app.emit("commit:done", &outcome);
    Ok(outcome)
}

#[tauri::command]
pub fn get_project_setting(key: String, state: State<'_, AppState>) -> AppResult<Option<String>> {
    let (db, _) = project(&state)?;
    committer::get_setting(&db, &key)
}

#[tauri::command]
pub fn set_project_setting(
    key: String,
    value: String,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (db, _) = project(&state)?;
    committer::set_setting(&db, &key, &value)
}
