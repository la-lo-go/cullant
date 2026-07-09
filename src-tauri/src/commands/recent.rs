use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct RecentEntry {
    pub(crate) path: String,
    last_opened: i64,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub path: String,
    pub last_opened: i64,
    pub available: bool,
}

const MAX_RECENT: usize = 24;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn recent_projects_path<R: Runtime>(app: &AppHandle<R>) -> Option<PathBuf> {
    let dir = app.path().app_data_dir().ok()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("recent_projects.json"))
}

/// Load the recent-projects list, most-recently-opened first. Never fails —
/// a missing or corrupt file just means an empty list.
pub(crate) fn load_recent<R: Runtime>(app: &AppHandle<R>) -> Vec<RecentEntry> {
    let Some(path) = recent_projects_path(app) else {
        return Vec::new();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return Vec::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn save_recent<R: Runtime>(app: &AppHandle<R>, list: &[RecentEntry]) {
    let Some(path) = recent_projects_path(app) else {
        return;
    };
    if let Ok(json) = serde_json::to_vec_pretty(list) {
        let _ = std::fs::write(path, json);
    }
}

/// Record that a project was just opened, moving it to the front of the
/// recent list (or inserting it). Called from `open_project` / the
/// CULLANT_OPEN_PROJECT startup hook — best-effort, errors are swallowed so a
/// broken app-data dir never blocks opening a project.
pub fn record_opened<R: Runtime>(app: &AppHandle<R>, path: &str) {
    let mut list = load_recent(app);
    list.retain(|e| e.path != path);
    list.insert(
        0,
        RecentEntry {
            path: path.to_string(),
            last_opened: unix_now(),
        },
    );
    list.truncate(MAX_RECENT);
    save_recent(app, &list);
}

/// Whether a remembered project is still reachable: a real directory on
/// desktop, or a still-granted SAF tree permission on Android.
fn is_available<R: Runtime>(app: &AppHandle<R>, path: &str) -> bool {
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        use tauri_plugin_saf::SafExt;
        return app.saf().check_tree_access(path).unwrap_or(false);
    }
    let _ = app;
    PathBuf::from(path).is_dir()
}

/// List recently-opened projects, most recent first, each flagged with
/// whether its folder is still present on disk.
#[tauri::command]
pub fn list_recent_projects(app: AppHandle) -> Vec<RecentProject> {
    load_recent(&app)
        .into_iter()
        .map(|e| RecentProject {
            available: is_available(&app, &e.path),
            path: e.path,
            last_opened: e.last_opened,
        })
        .collect()
}

/// Remove a project from the recent list only — never touches the folder or
/// its `.cullant` data on disk.
#[tauri::command]
pub fn remove_recent_project(app: AppHandle, path: String) {
    let mut list = load_recent(&app);
    list.retain(|e| e.path != path);
    save_recent(&app, &list);
}
