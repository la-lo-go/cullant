use std::sync::Arc;

use rusqlite::{params, OptionalExtension};
use tauri::State;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::AppState;

fn project_db(state: &AppState) -> AppResult<Arc<Db>> {
    let guard = state.project.lock().unwrap();
    Ok(guard.as_ref().ok_or(AppError::NoProject)?.db.clone())
}

/// The saved per-project UI session state as an opaque JSON string, or `None`
/// if nothing has been saved yet. The shape is owned by the frontend; the
/// backend stores and returns the blob verbatim.
#[tauri::command]
pub fn get_session_state(state: State<'_, AppState>) -> AppResult<Option<String>> {
    let db = project_db(&state)?;
    db.call(move |conn| {
        Ok(conn
            .query_row("SELECT state FROM session_state WHERE id = 1", [], |r| {
                r.get(0)
            })
            .optional()?)
    })
}

/// Persist the per-project UI session state (an opaque JSON blob from the
/// frontend). Upserts the single row.
#[tauri::command]
pub fn set_session_state(value: String, state: State<'_, AppState>) -> AppResult<()> {
    let db = project_db(&state)?;
    db.call(move |conn| {
        conn.execute(
            "INSERT INTO session_state (id, state) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET state = excluded.state",
            params![value],
        )?;
        Ok(())
    })
}
