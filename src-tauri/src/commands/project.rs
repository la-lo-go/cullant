use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::db::{migrations, Db};
use crate::error::{AppError, AppResult};
use crate::scan::ingest::PreviewMode;
use crate::store::ProjectStore;
use crate::thumbs::ThumbPool;
use crate::{scan, AppState, ProjectState};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectInfo {
    pub root_path: String,
    /// Friendly, human-readable project name derived from `root_path`
    /// (see [`project_display_name`]).
    pub display_name: String,
    pub db_path: String,
    pub schema_version: i64,
    pub file_count: i64,
}

/// Decode a percent-encoded string (`%20` -> space, `%3A` -> `:`), tolerating
/// malformed escapes by leaving them untouched. Never panics.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A friendly, human-readable name for a project given its stored identifier.
///
/// Desktop identifiers are filesystem paths — the leaf folder name is shown.
/// Android identifiers are SAF `content://` tree URIs whose last path segment
/// is a percent-encoded document id like `primary%3AFUJIFILM%20XT-4`; we decode
/// it, drop the volume prefix (`primary:`, `1A2B-3C4D:`, …) and show the last
/// segment of the remaining document path. Any malformed input falls back to a
/// reasonable string rather than panicking.
pub(crate) fn project_display_name(id: &str) -> String {
    if id.starts_with("content://") {
        // The SAF document id is the URI's last '/'-separated segment.
        let doc_id = id.rsplit('/').next().unwrap_or(id);
        let decoded = percent_decode(doc_id);
        // Strip the volume prefix ("primary:", storage-uuid, …), keeping the
        // relative document path.
        let doc = decoded.split_once(':').map_or(decoded.as_str(), |(_, p)| p);
        let name = doc.rsplit('/').find(|s| !s.is_empty()).unwrap_or(doc);
        return if name.is_empty() {
            id.to_string()
        } else {
            name.to_string()
        };
    }
    // Desktop filesystem path — the leaf folder name (handles both separators).
    let name = id.rsplit(['/', '\\']).find(|s| !s.is_empty()).unwrap_or(id);
    if name.is_empty() {
        id.to_string()
    } else {
        name.to_string()
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Kick off scan + the fused ingest pass (metadata + thumbnails + previews)
/// on a background thread; the UI is notified through scan:progress /
/// scan:done / metadata:done / thumbs:progress / thumbs:done /
/// previews:progress events.
fn spawn_scan(
    app: AppHandle,
    db: Arc<Db>,
    store: Arc<dyn ProjectStore>,
    root: PathBuf,
    mode: PreviewMode,
) {
    std::thread::Builder::new()
        .name("scanner".into())
        .spawn(move || {
            if let Err(e) = scan::scan_project(&app, &db, store.as_ref()) {
                tracing::error!("scan failed: {e}");
                let _ = app.emit("scan:error", e.to_string());
                return;
            }
            if let Err(e) = scan::ingest::run_ingest_pass(&app, &db, store.as_ref(), &root, mode) {
                tracing::error!("ingest pass failed: {e}");
            }
        })
        .expect("failed to spawn scanner thread");
}

#[tauri::command]
pub fn open_project(
    path: String,
    preview_mode: Option<PreviewMode>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectInfo> {
    do_open_project(&path, &app, &state, preview_mode.unwrap_or_default())
}

/// Launch the Android SAF folder picker and return the picked `content://` tree
/// URI (which `open_project` accepts as its `path`). `None` if the user
/// cancelled. On desktop this is a no-op returning `None` — the frontend uses
/// the native directory dialog there instead.
#[tauri::command]
pub fn pick_saf_tree(app: AppHandle) -> AppResult<Option<String>> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_saf::SafExt;
        match app.saf().open_tree() {
            Ok((tree_uri, _root)) => Ok(Some(tree_uri)),
            Err(e) => {
                tracing::info!("SAF picker returned no folder: {e}");
                Ok(None)
            }
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(None)
    }
}

/// Shared by the IPC command and the CULLANT_OPEN_PROJECT dev/startup hook.
pub fn do_open_project(
    path: &str,
    app: &AppHandle,
    state: &AppState,
    mode: PreviewMode,
) -> AppResult<ProjectInfo> {
    // On Android a project is a SAF `content://` tree, not a filesystem path.
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        return open_saf_project(path, app, state, mode);
    }

    let root = PathBuf::from(path);
    if !root.is_dir() {
        return Err(AppError::Other(format!("not a directory: {path}")));
    }
    // Reject Cullant's own sidecar dir: opening `<project>/.cullant` (or anything
    // nested inside it) as a project is nonsense — that folder holds the DB and
    // thumbnail cache, not the user's photos.
    if root
        .components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case(".cullant"))
    {
        return Err(AppError::Other(
            "This is Cullant's own data folder (.cullant), not a photo folder. \
             Pick the folder that contains your photos instead."
                .to_string(),
        ));
    }

    let db = Arc::new(Db::open(&root)?);
    let db_path = db.path().to_string_lossy().into_owned();
    let root_str = root.to_string_lossy().into_owned();
    let store: Arc<dyn ProjectStore> = Arc::new(crate::store::LocalFsStore::new(root.clone()));

    let root_for_db = root_str.clone();
    let (schema_version, file_count) = db.call(move |conn| {
        conn.execute(
            "INSERT INTO project (id, root_path, created_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
            params![root_for_db, unix_now()],
        )?;
        let version = migrations::current_version(conn)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |row| {
                row.get(0)
            })?;
        Ok((version, count))
    })?;

    let thumbs = ThumbPool::start(db.clone(), store.clone(), root.clone());
    *state.project.lock().unwrap() = Some(ProjectState {
        root: root.clone(),
        store: store.clone(),
        db: db.clone(),
        thumbs,
    });
    tracing::info!("opened project at {root_str}");
    crate::commands::recent::record_opened(app, &root_str);

    spawn_scan(app.clone(), db, store, root, mode);

    Ok(ProjectInfo {
        display_name: project_display_name(&root_str),
        root_path: root_str,
        db_path,
        schema_version,
        file_count,
    })
}

/// Real-filesystem base dir holding a project's `.cullant` sidecar (DB + thumb
/// cache), given its stored identifier: a filesystem path on desktop, a SAF
/// `content://` tree URI on Android (the latter maps to a private app dir keyed
/// by a hash of the URI, since SQLite cannot run inside a SAF tree).
pub(crate) fn project_data_base<R: tauri::Runtime>(
    app: &AppHandle<R>,
    id: &str,
) -> AppResult<PathBuf> {
    #[cfg(target_os = "android")]
    if id.starts_with("content://") {
        use tauri::Manager;
        let hash = xxhash_rust::xxh3::xxh3_64(id.as_bytes());
        return Ok(app
            .path()
            .app_data_dir()
            .map_err(|e| AppError::Other(format!("app_data_dir: {e}")))?
            .join("projects")
            .join(format!("{hash:016x}")));
    }
    let _ = app; // used only on Android
    Ok(PathBuf::from(id))
}

/// Open a SAF-backed project (Android). The picked `content://` tree hosts the
/// media; the DB and thumbnail cache live in private app storage keyed by a hash
/// of the tree URI, because SQLite cannot run inside a SAF tree.
#[cfg(target_os = "android")]
fn open_saf_project(
    tree_uri: &str,
    app: &AppHandle,
    state: &AppState,
    mode: PreviewMode,
) -> AppResult<ProjectInfo> {
    use tauri_plugin_saf::SafExt;

    let root_doc = app
        .saf()
        .root_document_id(tree_uri)
        .map_err(|e| AppError::Other(format!("saf: {e}")))?;

    let base = project_data_base(app, tree_uri)?;
    std::fs::create_dir_all(&base)?;

    let db = Arc::new(Db::open(&base)?);
    let db_path = db.path().to_string_lossy().into_owned();
    let store: Arc<dyn ProjectStore> = Arc::new(crate::store::SafStore::new(
        tree_uri.to_string(),
        root_doc,
        app.clone(),
        db.clone(),
    ));

    let tree_owned = tree_uri.to_string();
    let (schema_version, file_count) = db.call(move |conn| {
        conn.execute(
            "INSERT INTO project (id, root_path, created_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET root_path = excluded.root_path",
            params![tree_owned, unix_now()],
        )?;
        let version = migrations::current_version(conn)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |row| {
                row.get(0)
            })?;
        Ok((version, count))
    })?;

    let thumbs = ThumbPool::start(db.clone(), store.clone(), base.clone());
    *state.project.lock().unwrap() = Some(ProjectState {
        root: base.clone(),
        store: store.clone(),
        db: db.clone(),
        thumbs,
    });
    tracing::info!("opened SAF project {tree_uri}");
    crate::commands::recent::record_opened(app, tree_uri);

    spawn_scan(app.clone(), db, store, base, mode);

    Ok(ProjectInfo {
        display_name: project_display_name(tree_uri),
        root_path: tree_uri.to_string(),
        db_path,
        schema_version,
        file_count,
    })
}

/// Project already open in this session, if any (used by the frontend on
/// startup, e.g. after an auto-open via CULLANT_OPEN_PROJECT).
#[tauri::command]
pub fn current_project(state: State<'_, AppState>) -> AppResult<Option<ProjectInfo>> {
    let (db, root) = {
        let guard = state.project.lock().unwrap();
        match guard.as_ref() {
            None => return Ok(None),
            Some(p) => (p.db.clone(), p.root.clone()),
        }
    };
    let db_path = db.path().to_string_lossy().into_owned();
    let (schema_version, file_count) = db.call(|conn| {
        let version = migrations::current_version(conn)?;
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM files WHERE status = 0", [], |row| {
                row.get(0)
            })?;
        Ok((version, count))
    })?;
    let root_path = root.to_string_lossy().into_owned();
    Ok(Some(ProjectInfo {
        display_name: project_display_name(&root_path),
        root_path,
        db_path,
        schema_version,
        file_count,
    }))
}

#[tauri::command]
pub fn rescan_project(
    preview_mode: Option<PreviewMode>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let (db, store, root) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(AppError::NoProject)?;
        (
            project.db.clone(),
            project.store.clone(),
            project.root.clone(),
        )
    };
    spawn_scan(app, db, store, root, preview_mode.unwrap_or_default());
    Ok(())
}

#[tauri::command]
pub fn close_project(state: State<'_, AppState>) {
    *state.project.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use super::{percent_decode, project_display_name};

    #[test]
    fn saf_uri_yields_friendly_name() {
        assert_eq!(
            project_display_name(
                "content://com.android.externalstorage.documents/tree/primary%3AFUJIFILM%20XT-4"
            ),
            "FUJIFILM XT-4"
        );
        // A subfolder document path shows its last segment.
        assert_eq!(
            project_display_name(
                "content://com.android.externalstorage.documents/tree/primary%3ADCIM%2FCamera"
            ),
            "Camera"
        );
        // Non-primary storage volume (SD card uuid) prefix is stripped too.
        assert_eq!(
            project_display_name(
                "content://com.android.externalstorage.documents/tree/1A2B-3C4D%3APhotos"
            ),
            "Photos"
        );
    }

    #[test]
    fn desktop_path_yields_leaf_folder() {
        assert_eq!(project_display_name(r"C:\Users\lalo\Pictures\Trip"), "Trip");
        assert_eq!(project_display_name("/home/lalo/Pictures/Trip"), "Trip");
        // A trailing separator does not blank out the name.
        assert_eq!(project_display_name(r"D:\Shoots\Wedding\"), "Wedding");
    }

    #[test]
    fn malformed_input_falls_back_gracefully() {
        // Dangling / invalid percent escapes are left intact, no panic.
        assert_eq!(
            project_display_name("content://x/tree/primary%3ABad%2"),
            "Bad%2"
        );
        // Empty document path -> fall back to the raw id rather than "".
        assert_eq!(
            project_display_name("content://x/tree/primary%3A"),
            "content://x/tree/primary%3A"
        );
        assert_eq!(percent_decode("%ZZbad"), "%ZZbad");
    }
}
