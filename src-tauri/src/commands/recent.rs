use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

use crate::error::{AppError, AppResult};

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct RecentEntry {
    pub(crate) path: String,
    last_opened: i64,
    /// Storage kind captured while the volume was connected, so the gallery can
    /// still show the right badge once it's disconnected — a probe of an absent
    /// device can't classify it. Absent in older files (defaults to `None`).
    #[serde(default)]
    kind: Option<crate::storage::StorageKind>,
    /// Friendly volume name captured while connected (same rationale).
    #[serde(default)]
    volume_name: Option<String>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RecentProject {
    pub path: String,
    /// Friendly, human-readable name derived from `path` — the leaf folder name
    /// for desktop paths, a decoded label for Android SAF `content://` URIs.
    pub display_name: String,
    pub last_opened: i64,
    /// Back-compat flag: true iff `storage.state` is `ok`.
    pub available: bool,
    /// Probed storage status (present / disconnected volume / folder gone) plus
    /// storage kind and a friendly volume name, for the gallery badge.
    pub storage: crate::storage::StorageInfo,
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
    // The project is connected right now, so capture its storage kind + friendly
    // name to fall back on when it's later disconnected.
    let info = crate::storage::probe(app, path);
    let mut list = load_recent(app);
    list.retain(|e| e.path != path);
    list.insert(
        0,
        RecentEntry {
            path: path.to_string(),
            last_opened: unix_now(),
            kind: Some(info.kind),
            volume_name: info.volume_name,
        },
    );
    list.truncate(MAX_RECENT);
    save_recent(app, &list);
}

/// List recently-opened projects, most recent first, each with its probed
/// storage status (present / disconnected volume / folder not found).
#[tauri::command]
pub fn list_recent_projects(app: AppHandle) -> Vec<RecentProject> {
    load_recent(&app)
        .into_iter()
        .map(|e| {
            let mut storage = crate::storage::probe(&app, &e.path);
            // When the live probe can't classify the volume (typically because
            // it's disconnected), fall back to the kind/name captured while it
            // was connected.
            if storage.kind == crate::storage::StorageKind::Unknown {
                if let Some(k) = e.kind {
                    storage.kind = k;
                }
            }
            if storage.state != crate::storage::StorageState::Ok && e.volume_name.is_some() {
                storage.volume_name = e.volume_name.clone();
            }
            RecentProject {
                available: storage.available(),
                storage,
                display_name: super::project::project_display_name(&e.path),
                path: e.path,
                last_opened: e.last_opened,
            }
        })
        .collect()
}

/// Probe the storage backing an arbitrary project identifier — used to watch the
/// currently-open project's folder/volume while it's in use, so the UI can warn
/// when it's disconnected or removed.
#[tauri::command]
pub fn probe_storage(app: AppHandle, id: String) -> crate::storage::StorageInfo {
    crate::storage::probe(&app, &id)
}

/// Fully forget a project: drop it from the recent list AND delete Cullant's
/// own data (the SQLite DB + thumbnail cache). The user's photos are NEVER
/// touched — only Cullant's sidecar is removed.
///
/// Desktop: the sidecar is `<project>/.cullant`; it goes to the OS recycle bin
/// (reversible) when possible, falling back to a permanent remove otherwise.
/// Android: the data lives in a private app dir (`project_data_base`) outside
/// the picked SAF tree, so it is removed directly.
#[tauri::command]
pub fn delete_project_data(app: AppHandle, path: String) -> AppResult<()> {
    // Forget it from the recent list regardless of what happens to the data.
    let mut list = load_recent(&app);
    list.retain(|e| e.path != path);
    save_recent(&app, &list);

    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        let base = super::project::project_data_base(&app, &path)?;
        if base.is_dir() {
            std::fs::remove_dir_all(&base)
                .map_err(|e| AppError::Other(format!("delete project data: {e}")))?;
        }
        return Ok(());
    }

    // Desktop (and any non-SAF path): the sidecar is `<project>/.cullant`.
    let data_dir = PathBuf::from(&path).join(".cullant");
    if data_dir.is_dir() {
        delete_sidecar(&data_dir)?;
    }
    Ok(())
}

/// Remove Cullant's `.cullant` sidecar directory. Prefers the OS recycle bin so
/// the deletion is reversible; falls back to a permanent recursive remove if the
/// platform can't trash a directory.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn delete_sidecar(dir: &std::path::Path) -> AppResult<()> {
    if trash::delete(dir).is_ok() {
        return Ok(());
    }
    std::fs::remove_dir_all(dir).map_err(|e| AppError::Other(format!("delete project data: {e}")))
}

/// Mobile fallback: no OS recycle bin, so remove the sidecar permanently.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn delete_sidecar(dir: &std::path::Path) -> AppResult<()> {
    std::fs::remove_dir_all(dir).map_err(|e| AppError::Other(format!("delete project data: {e}")))
}
