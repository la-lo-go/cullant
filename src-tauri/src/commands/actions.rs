use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::db::Db;
use crate::engine::actions::{self, ActionKind, PairScope, PendingAction};
use crate::engine::committer::{self, CommitOutcome, CommitPlan};
use crate::engine::culling::Targets;
use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;
use crate::AppState;

fn project(state: &AppState) -> AppResult<(Arc<Db>, Arc<dyn ProjectStore>)> {
    let guard = state.project.lock().unwrap();
    let p = guard.as_ref().ok_or(AppError::NoProject)?;
    Ok((p.db.clone(), p.store.clone()))
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
pub fn remove_pending_for_files(
    targets: Targets,
    action: ActionKind,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<usize> {
    let (db, _) = project(&state)?;
    let n = actions::remove_for_files(&db, targets, action)?;
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

/// Number of photos with a pending XMP sidecar write. Separate from the pending-
/// actions queue (XMP dirtiness is a per-file flag), so the toolbar queries it
/// alongside `list_pending` to decide whether the commit button has work.
#[tauri::command]
pub fn xmp_dirty_count(state: State<'_, AppState>) -> AppResult<i64> {
    let (db, _) = project(&state)?;
    committer::xmp_dirty_count(&db)
}

#[tauri::command]
pub fn commit_preview(state: State<'_, AppState>) -> AppResult<CommitPlan> {
    let (db, store) = project(&state)?;
    committer::preview(&db, store.as_ref())
}

#[tauri::command]
pub fn commit_execute(
    plan_hash: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CommitOutcome> {
    let (db, store) = project(&state)?;
    let progress_app = app.clone();
    let outcome = committer::execute(
        &db,
        store.as_ref(),
        &plan_hash,
        move |phase, done, total| {
            let _ = progress_app.emit(
                "commit:progress",
                serde_json::json!({ "phase": phase, "done": done, "total": total }),
            );
        },
    )?;
    let _ = app.emit("pending:changed", ());
    let _ = app.emit("commit:done", &outcome);
    Ok(outcome)
}

/// Commit a single section (deletes / moves / copies / xmp) — the dialog's
/// hold-to-run buttons. Validates only that section's digest, so the untouched
/// sections stay pending with their own still-valid hashes.
#[tauri::command]
pub fn commit_execute_section(
    section: String,
    section_hash: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<CommitOutcome> {
    let (db, store) = project(&state)?;
    let progress_app = app.clone();
    let outcome = committer::execute_section(
        &db,
        store.as_ref(),
        &section,
        &section_hash,
        move |phase, done, total| {
            let _ = progress_app.emit(
                "commit:progress",
                serde_json::json!({ "phase": phase, "done": done, "total": total }),
            );
        },
    )?;
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
