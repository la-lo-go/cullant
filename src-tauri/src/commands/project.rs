use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::params;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands::import_notify::{End, ImportNotifyGuard};
use crate::db::{migrations, Db};
use crate::error::{AppError, AppResult};
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
    /// True when the live database is private and the app copies it into the
    /// project folder (Android).
    pub mirrors_metadata: bool,
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

/// Clears `AppState::scan_active` however the scanner thread ends — including
/// the early returns for a failed scan and an empty folder.
struct ScanActive(AppHandle, u64);

impl Drop for ScanActive {
    fn drop(&mut self) {
        let state = self.0.state::<AppState>();
        let guard = state.project.lock().unwrap();
        if guard.as_ref().is_some_and(|p| p.db.generation() == self.1) {
            state
                .scan_active
                .store(false, std::sync::atomic::Ordering::Release);
        }
    }
}

/// Kick off scan + the two-phase ingest pass (metadata gate, then background
/// thumbnails + previews) on a background thread; the UI is notified through
/// scan:progress / scan:done / scan:empty / metadata:progress / metadata:done /
/// thumbs:progress / thumbs:done / previews:progress events.
///
/// `initial_open_id` is `Some(identifier)` (the desktop path or Android SAF
/// tree URI used by the recent-projects list) only when this scan is the
/// first one right after opening a project — never on a plain rescan. When
/// set and the scan finds zero recognized photos/videos, the just-opened
/// project is rolled back entirely (closed, forgotten from recents, its
/// freshly-created `.cullant` sidecar removed) rather than left open on an
/// empty grid, and `scan:empty` fires instead of running the ingest pass.
fn spawn_scan(
    app: AppHandle,
    db: Arc<Db>,
    store: Arc<dyn ProjectStore>,
    root: PathBuf,
    thumbs: Arc<ThumbPool>,
    initial_open_id: Option<String>,
) -> AppResult<()> {
    app.state::<AppState>()
        .scan_active
        .store(true, std::sync::atomic::Ordering::Release);
    let active = ScanActive(app.clone(), db.generation());
    let notify = initial_open_id.as_ref().map(|id| {
        ImportNotifyGuard::start(
            &app,
            project_display_name(id),
            [db.project_root(), id.clone()],
        )
    });
    std::thread::Builder::new()
        .name("scanner".into())
        .spawn(move || {
            let _active = active;
            // Declared after `_active`, so it drops first and sees the final scan state.
            let mut notify = notify;
            let project_root = db.project_root();
            let error_app = app.clone();
            // Last line of defence: whatever panics in the walk or the ingest,
            // the frontend hears `scan:error` and drops its progress bars instead
            // of showing a phase that will never finish.
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_scan(app, db, store, root, thumbs, initial_open_id)
            }));
            let end = outcome.unwrap_or_else(|payload| {
                let message = format!("internal error: {}", crate::panic_message(payload.as_ref()));
                tracing::error!("scanner panicked: {message}");
                emit_scan_error(&error_app, &project_root, message.clone());
                Some(End::Error(message))
            });
            if let Some(notify) = notify.as_mut() {
                notify.set_end(end);
            }
        })
        .map_err(|error| AppError::Other(format!("could not start scanner: {error}")))?;
    Ok(())
}

fn run_scan(
    app: AppHandle,
    db: Arc<Db>,
    store: Arc<dyn ProjectStore>,
    root: PathBuf,
    thumbs: Arc<ThumbPool>,
    initial_open_id: Option<String>,
) -> Option<End> {
    let cancelled = || thumbs.is_shut_down();
    let done = match scan::scan_project(&app, &db, store.as_ref(), &cancelled) {
        Ok(done) => done,
        Err(AppError::Cancelled) => return None,
        Err(_) if cancelled() => return None,
        Err(error) => {
            emit_scan_error(&app, &db.project_root(), error.to_string());
            return Some(End::Error(error.to_string()));
        }
    };
    // After the walk, not before it: the cache of a file the previous scan
    // marked missing is kept when the file has come back. Off the open path too,
    // so a large cache no longer delays the grid.
    if let Err(error) = crate::thumbs::prune_cache(&db, &root) {
        tracing::warn!("could not prune the thumbnail cache: {error}");
    }
    if done.file_count == 0 {
        if let Some(id) = initial_open_id {
            let result = rollback_fresh_empty(&app, db, store, root, thumbs);
            return empty_end(&app, &id, result);
        }
    }
    match scan::ingest::run_ingest_pass(&app, &db, store.as_ref(), &thumbs) {
        Ok(_) => Some(End::Done(done.file_count.max(0) as u64)),
        Err(_) if thumbs.is_shut_down() => None,
        Err(error) => {
            emit_scan_error(&app, &db.project_root(), error.to_string());
            Some(End::Error(error.to_string()))
        }
    }
}

fn empty_end(app: &AppHandle, id: &str, rollback: AppResult<bool>) -> Option<End> {
    match rollback {
        Ok(true) => {
            let _ = crate::commands::recent::delete_project_data(app.clone(), id.to_string());
            let _ = app.emit("scan:empty", serde_json::json!({"projectRoot": id}));
            Some(End::Empty)
        }
        Ok(false) => None,
        Err(error) => {
            emit_scan_error(app, id, error.to_string());
            Some(End::Error(error.to_string()))
        }
    }
}

// Only a data directory created by this open operation can reach this path.
fn rollback_fresh_empty(
    app: &AppHandle,
    db: Arc<Db>,
    store: Arc<dyn ProjectStore>,
    root: PathBuf,
    thumbs: Arc<ThumbPool>,
) -> AppResult<bool> {
    let state = app.state::<AppState>();
    // An open of this same folder must not run between the generation check
    // and the removal of the `.cullant` directory it would be using.
    let _lifecycle = lifecycle_lock(&state);
    let previous = {
        let mut guard = state.project.lock().unwrap();
        if guard
            .as_ref()
            .is_none_or(|p| p.db.generation() != db.generation())
        {
            return Ok(false);
        }
        guard.take()
    };
    if let Some(previous) = previous {
        previous.thumbs.shutdown();
    }
    state
        .scan_active
        .store(false, std::sync::atomic::Ordering::Release);
    drop(thumbs);
    drop(store);
    drop(db);
    remove_dir_with_retry(&root.join(".cullant"))?;
    Ok(true)
}

fn emit_scan_error(app: &AppHandle, project_root: &str, error: String) {
    let _ = app.emit(
        "scan:error",
        serde_json::json!({"projectRoot": project_root, "error": error}),
    );
}

/// Runs off the main thread (`async`): opening shuts down the previous
/// project's workers, which waits for the decode each one is in, and opens a
/// database. On the main thread that froze the window on desktop and blocked the
/// looper every SAF call and event goes through on Android.
#[tauri::command(async)]
pub fn open_project(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<ProjectInfo> {
    let _lifecycle = lifecycle_lock(&state);
    do_open_project(&path, &app, &state)
}

fn lifecycle_lock(state: &AppState) -> std::sync::MutexGuard<'_, ()> {
    // A panic while holding it leaves nothing half-done that the next caller
    // cannot handle: every step re-reads `project` under its own lock.
    state.lifecycle.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command(async)]
pub fn folder_project_path(path: String, state: State<'_, AppState>) -> AppResult<String> {
    let store = {
        let guard = state.project.lock().unwrap();
        guard.as_ref().ok_or(AppError::NoProject)?.store.clone()
    };
    store.folder_project_path(&path)
}

/// Launch the Android SAF folder picker and return the picked `content://` tree
/// URI (which `open_project` accepts as its `path`). `None` if the user
/// cancelled. On desktop this is a no-op returning `None` — the frontend uses
/// the native directory dialog there instead.
#[tauri::command]
pub fn pick_saf_tree(app: AppHandle, prefer_removable: Option<bool>) -> AppResult<Option<String>> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_saf::SafExt;
        match app.saf().open_tree(prefer_removable.unwrap_or(false)) {
            Ok((tree_uri, _root)) => Ok(Some(tree_uri)),
            Err(tauri_plugin_saf::Error::Cancelled) => Ok(None),
            Err(e) => Err(crate::error::AppError::Other(e.to_string())),
        }
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (app, prefer_removable);
        Ok(None)
    }
}

/// Return true once for an Android USB-attach intent delivered to Cullant.
#[tauri::command]
pub fn consume_usb_attach(app: AppHandle) -> AppResult<bool> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_saf::SafExt;
        app.saf()
            .consume_usb_attach()
            .map_err(|e| AppError::Other(format!("saf: {e}")))
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = app;
        Ok(false)
    }
}

pub fn do_open_project(path: &str, app: &AppHandle, state: &AppState) -> AppResult<ProjectInfo> {
    // On Android a project is a SAF `content://` tree, not a filesystem path.
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        return open_saf_project(path, app, state);
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
            "This is Cullant's own data folder (.cullant), not a photo or video folder. \
             Pick the folder that contains your photos or videos instead."
                .to_string(),
        ));
    }

    let fresh_data = !root.join(".cullant").exists();
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
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status = 0 AND kind IN (0, 1, 2)",
            [],
            |row| row.get(0),
        )?;
        Ok((version, count))
    })?;

    close_project_inner(state);
    let thumbs = Arc::new(ThumbPool::start(db.clone(), store.clone(), root.clone()));
    {
        // Replacing an already-open project must stop its background generation
        // too (same reason as close_project), or two pools grind at once.
        let mut guard = state.project.lock().unwrap();
        if let Some(prev) = guard.take() {
            prev.thumbs.shutdown();
        }
        *guard = Some(ProjectState {
            root: root.clone(),
            store: store.clone(),
            db: db.clone(),
            thumbs: thumbs.clone(),
        });
    }
    tracing::info!("opened project at {root_str}");
    crate::commands::recent::record_opened(app, &root_str);

    spawn_scan(
        app.clone(),
        db,
        store,
        root,
        thumbs,
        fresh_data.then(|| root_str.clone()),
    )?;

    Ok(ProjectInfo {
        display_name: project_display_name(&root_str),
        root_path: root_str,
        db_path,
        schema_version,
        file_count,
        mirrors_metadata: false,
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
fn open_saf_project(tree_uri: &str, app: &AppHandle, state: &AppState) -> AppResult<ProjectInfo> {
    use tauri_plugin_saf::SafExt;

    let root_doc = app
        .saf()
        .root_document_id(tree_uri)
        .map_err(|e| AppError::Other(format!("saf: {e}")))?;

    let base = project_data_base(app, tree_uri)?;
    std::fs::create_dir_all(&base)?;

    let fresh_data = !base.join(".cullant").exists();
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
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status = 0 AND kind IN (0, 1, 2)",
            [],
            |row| row.get(0),
        )?;
        Ok((version, count))
    })?;

    close_project_inner(state);
    let thumbs = Arc::new(ThumbPool::start(db.clone(), store.clone(), base.clone()));
    {
        // Stop a previously-open project's background generation before replacing.
        let mut guard = state.project.lock().unwrap();
        if let Some(prev) = guard.take() {
            prev.thumbs.shutdown();
        }
        *guard = Some(ProjectState {
            root: base.clone(),
            store: store.clone(),
            db: db.clone(),
            thumbs: thumbs.clone(),
        });
    }
    tracing::info!("opened SAF project {tree_uri}");
    crate::commands::recent::record_opened(app, tree_uri);

    spawn_scan(
        app.clone(),
        db,
        store,
        base,
        thumbs,
        fresh_data.then(|| tree_uri.to_string()),
    )?;

    Ok(ProjectInfo {
        display_name: project_display_name(tree_uri),
        root_path: tree_uri.to_string(),
        db_path,
        schema_version,
        file_count,
        mirrors_metadata: true,
    })
}

#[tauri::command]
pub fn current_project(state: State<'_, AppState>) -> AppResult<Option<ProjectInfo>> {
    let (db, mirrors_metadata) = {
        let guard = state.project.lock().unwrap();
        match guard.as_ref() {
            None => return Ok(None),
            Some(p) => (p.db.clone(), p.store.mirrors_metadata()),
        }
    };
    let db_path = db.path().to_string_lossy().into_owned();
    let (schema_version, file_count) = db.call(|conn| {
        let version = migrations::current_version(conn)?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE status = 0 AND kind IN (0, 1, 2)",
            [],
            |row| row.get(0),
        )?;
        Ok((version, count))
    })?;
    let root_path = db.project_root();
    Ok(Some(ProjectInfo {
        display_name: project_display_name(&root_path),
        root_path,
        db_path,
        schema_version,
        file_count,
        mirrors_metadata,
    }))
}

/// Return how many present files still lack metadata. A zero count means the
/// grid is safe to show, even if the `metadata:done` event was missed.
#[tauri::command]
pub fn ingest_pending(state: State<'_, AppState>) -> AppResult<i64> {
    let db = state.project.lock().unwrap().as_ref().map(|p| p.db.clone());
    let Some(db) = db else { return Ok(0) };
    db.call(|conn| {
        Ok(conn.query_row(
            "SELECT COUNT(*) FROM files
             WHERE status = 0 AND kind IN (0, 1, 2) AND capture_time IS NULL",
            [],
            |r| r.get(0),
        )?)
    })
}

#[tauri::command]
pub fn rescan_project(app: AppHandle, state: State<'_, AppState>) -> AppResult<()> {
    let (db, store, root, thumbs) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(AppError::NoProject)?;
        if state
            .scan_active
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .is_err()
        {
            return Err(AppError::Other(
                "Cullant is still reading this project. Wait for it to finish.".into(),
            ));
        }
        (
            project.db.clone(),
            project.store.clone(),
            project.root.clone(),
            project.thumbs.clone(),
        )
    };
    spawn_scan(app, db, store, root, thumbs, None)
}

/// Forget everything Cullant has stored about the open project and read the
/// folder again from nothing.
///
/// The photos are never touched. What goes is Cullant's own data directory —
/// the database (ratings, flags, labels, tags, the pending queue and the commit
/// history) and the generated thumbnail and preview cache. On desktop that is
/// `<project>/.cullant`; on Android it is the private app directory holding the
/// same things, which is why the path comes from the database's own location
/// rather than from an assumption about where it lives.
///
/// XMP sidecars survive on disk, so a project exported to them comes back with
/// its ratings, flags and labels — the rescan reads them in like any other
/// library arriving from elsewhere.
#[tauri::command(async)]
pub fn reimport_project(app: AppHandle, state: State<'_, AppState>) -> AppResult<ProjectInfo> {
    let _lifecycle = lifecycle_lock(&state);
    if state.scan_active.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Other(
            "Cullant is still reading this project. Wait for it to finish, then try again.".into(),
        ));
    }

    let (root_path, data_dir) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(AppError::NoProject)?;
        // The string this project was opened with: a folder on desktop, a SAF
        // tree URI on Android. Reopening has to use the same one.
        let root_path: String = project.db.call_read(|conn| {
            Ok(
                conn.query_row("SELECT root_path FROM project WHERE id = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })?;
        let data_dir = project
            .db
            .path()
            .parent()
            .ok_or_else(|| AppError::Other("project database has no directory".into()))?
            .to_path_buf();
        (root_path, data_dir)
    };

    // Close first, and completely: the database file cannot be removed while its
    // writer thread still holds it open, and the thumbnail pool would keep
    // writing into a directory that is going away.
    close_project_inner(&state);

    if data_dir.exists() {
        remove_dir_with_retry(&data_dir)?;
    }

    do_open_project(&root_path, &app, &state)
}

/// Windows refuses to unlink a file anything still holds open, and SQLite
/// releases its `-wal`/`-shm` companions a moment after the connection itself
/// goes. The workers are joined before we get here, so a failure now is that
/// last moment rather than a leak — the same reason `Db::open` retries.
fn remove_dir_with_retry(dir: &std::path::Path) -> AppResult<()> {
    const ATTEMPTS: u32 = 10;
    let mut last: Option<std::io::Error> = None;
    for attempt in 1..=ATTEMPTS {
        match std::fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) => {
                if attempt < ATTEMPTS {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                last = Some(e);
            }
        }
    }
    Err(AppError::Other(format!(
        "could not remove {}: {}",
        dir.to_string_lossy(),
        last.map(|e| e.to_string()).unwrap_or_default()
    )))
}

fn close_project_inner(state: &AppState) {
    // Take the project out AND shut its ThumbPool down. Clearing the state alone
    // is not enough: the scanner thread running ingest Phase B holds its own
    // `Arc<ThumbPool>` clone, so without an explicit shutdown the background
    // thumbnail/preview/video generation keeps running after the user has
    // returned to the home screen. shutdown() drains the pending queue (releasing
    // the scanner's completion channel) and stops the workers.
    // Taken out first: dropping the project waits for its database to close,
    // and the project mutex must not be held meanwhile.
    let previous = state.project.lock().unwrap().take();
    if let Some(prev) = previous {
        prev.thumbs.shutdown();
    }
    // Cache keys embed file ids, which are only unique within one project's
    // database, so the next project must not inherit any of them.
    crate::thumbs::memcache::clear();
    state
        .scan_active
        .store(false, std::sync::atomic::Ordering::Release);
}

#[tauri::command(async)]
pub fn close_project(state: State<'_, AppState>) -> AppResult<()> {
    let _lifecycle = lifecycle_lock(&state);
    close_project_inner(&state);
    Ok(())
}

/// Set the preview-quality preference without discarding existing previews.
/// Returns whether a scan is running. That scan restages the old previews
/// before it ends, so the caller must not start a second one.
#[tauri::command]
pub fn set_preview_quality(state: State<'_, AppState>, long_edge: u32) -> bool {
    crate::thumbs::set_preview_long_edge(long_edge);
    state.scan_active.load(std::sync::atomic::Ordering::Acquire)
}

/// The sizes Settings may offer, so the UI never invents one the backend
/// rejects.
#[tauri::command]
pub fn preview_quality_choices() -> Vec<u32> {
    crate::thumbs::PREVIEW_LONG_EDGE_CHOICES.to_vec()
}

/// Throw away every generated loupe preview for the open project: the cached
/// JPEGs first, then the rows pointing at them. Grid thumbnails, video posters
/// and `Full` renders are untouched, and no user file is ever involved.
///
/// Test hook: the E2E scripts use it to force a cold preview cache. Refuses
/// while a scan is running rather than deleting rows the ingest pass is writing.
#[tauri::command(async)]
pub fn discard_previews(state: State<'_, AppState>) -> AppResult<usize> {
    if state.scan_active.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(AppError::Other(
            "Cullant is still reading this project. Wait for it to finish, then try again.".into(),
        ));
    }
    let (db, root) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(AppError::NoProject)?;
        (project.db.clone(), project.root.clone())
    };

    let paths: Vec<String> = db.call_read(|conn| {
        let mut stmt =
            conn.prepare("SELECT cache_path FROM thumbnails WHERE kind = 1 AND cache_path <> ''")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    })?;

    // Files first: a row that outlives its file is regenerated, a file that
    // outlives its row leaks until the project is deleted.
    let thumbs_dir = root.join(".cullant").join("thumbs");
    let mut removed = 0usize;
    for rel in &paths {
        if std::fs::remove_file(thumbs_dir.join(rel)).is_ok() {
            removed += 1;
        }
    }
    db.call(|conn| {
        conn.execute("DELETE FROM thumbnails WHERE kind = 1", [])?;
        Ok(())
    })?;
    tracing::info!("discarded {removed} of {} cached previews", paths.len());
    Ok(removed)
}

/// Set the video-thumbnail preference. The ingest pass reads it before its
/// final video-poster tier.
#[tauri::command]
pub fn set_generate_video_thumbs(on: bool, state: State<'_, AppState>) {
    state
        .generate_video_thumbs
        .store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Copy the live database into the project folder when the store cannot keep
/// it there. Reports `written`, `unchanged` or `conflict`.
#[tauri::command(async)]
pub fn export_folder_copy(
    state: State<'_, AppState>,
) -> AppResult<crate::engine::mirror::ExportOutcome> {
    let _lifecycle = lifecycle_lock(&state);
    let (db, store, base) = {
        let guard = state.project.lock().unwrap();
        let p = guard.as_ref().ok_or(AppError::NoProject)?;
        (p.db.clone(), p.store.clone(), p.root.clone())
    };
    crate::engine::mirror::export_folder_copy(&db, store.as_ref(), &base)
}

/// What the project folder holds in `.cullant/cullant.db`, for a store that
/// mirrors the database into the folder.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub struct FolderCopyInfo {
    folder_copy: Option<FolderCopyStat>,
    /// This device already has a live database for the folder.
    has_local: bool,
    /// This device wrote the copy. The user has nothing to decide.
    is_own: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub struct FolderCopyStat {
    #[serde(flatten)]
    fingerprint: crate::engine::mirror::Fingerprint,
}

fn not_mirrored() -> AppError {
    AppError::Other("this project keeps its database in the folder".into())
}

/// Before an open: does the folder hold a database this device did not write?
/// `None` for a path that is not a mirroring store. It never opens the project.
#[tauri::command(async)]
pub fn inspect_folder_copy(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<Option<FolderCopyInfo>> {
    let _lifecycle = lifecycle_lock(&state);
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        return saf_copy::inspect(&path, &app).map(Some);
    }
    let _ = (path, app);
    Ok(None)
}

/// The user kept this device's data. The folder copy with `fingerprint`, as
/// inspect showed it, stops asking. A copy changed since then still asks.
#[tauri::command(async)]
pub fn accept_local_data(
    path: String,
    fingerprint: crate::engine::mirror::Fingerprint,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _lifecycle = lifecycle_lock(&state);
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        return saf_copy::accept(&path, fingerprint, &app);
    }
    let _ = (path, fingerprint, app);
    Err(not_mirrored())
}

/// Replace this device's data with the folder copy. The caller opens the
/// project next.
#[tauri::command(async)]
pub fn import_folder_copy(
    path: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> AppResult<()> {
    let _lifecycle = lifecycle_lock(&state);
    #[cfg(target_os = "android")]
    if path.starts_with("content://") {
        return saf_copy::import(&path, &app, &state);
    }
    let _ = (path, app);
    Err(not_mirrored())
}

/// The SAF reads behind the three commands above. They run before the project
/// is open, so they cannot use `SafStore`, which needs the live database.
#[cfg(target_os = "android")]
mod saf_copy {
    use std::path::Path;

    use tauri_plugin_saf::{SafEntry, SafExt};

    use super::*;
    use crate::engine::mirror;

    struct Found {
        db: Option<SafEntry>,
        wal: Option<SafEntry>,
    }

    fn saf_err(e: tauri_plugin_saf::Error) -> AppError {
        AppError::Other(format!("saf: {e}"))
    }

    fn find(app: &AppHandle, tree: &str) -> AppResult<Found> {
        let saf = app.saf();
        let root = saf.root_document_id(tree).map_err(saf_err)?;
        let top = saf.list_children(tree, &root).map_err(saf_err)?;
        let Some(dir) = top.into_iter().find(|e| e.is_dir && e.name == ".cullant") else {
            return Ok(Found {
                db: None,
                wal: None,
            });
        };
        let mut files = saf.list_children(tree, &dir.document_id).map_err(saf_err)?;
        let mut take = |name: &str| {
            let at = files.iter().position(|e| !e.is_dir && e.name == name)?;
            Some(files.swap_remove(at))
        };
        Ok(Found {
            db: take("cullant.db"),
            wal: take("cullant.db-wal"),
        })
    }

    fn copy_out(app: &AppHandle, tree: &str, entry: &SafEntry, dest: &Path) -> AppResult<()> {
        let mut source = app
            .saf()
            .open_file(tree, &entry.document_id, "r")
            .map_err(saf_err)?;
        std::io::copy(&mut source, &mut std::fs::File::create(dest)?)?;
        Ok(())
    }

    /// The copy and its WAL, as `cullant.db` and `cullant.db-wal` in `staging`.
    fn stage(
        app: &AppHandle,
        tree: &str,
        db: &SafEntry,
        wal: Option<&SafEntry>,
        staging: &Path,
    ) -> AppResult<()> {
        copy_out(app, tree, db, &staging.join("cullant.db"))?;
        if let Some(wal) = wal {
            copy_out(app, tree, wal, &staging.join("cullant.db-wal"))?;
        }
        Ok(())
    }

    fn stat(entry: &SafEntry) -> (i64, i64) {
        // Seconds, as `ProjectStore::stat` reports them.
        (entry.size as i64, entry.mtime / 1000)
    }

    fn fingerprint(db: &SafEntry, wal: Option<&SafEntry>) -> mirror::Fingerprint {
        let (size, mtime) = stat(db);
        mirror::Fingerprint {
            size,
            mtime,
            wal: mirror::wal_fingerprint(wal.map(stat)),
        }
    }

    pub fn inspect(tree: &str, app: &AppHandle) -> AppResult<FolderCopyInfo> {
        let base = project_data_base(app, tree)?;
        let has_local = base.join(".cullant").join("cullant.db").exists();
        let found = find(app, tree)?;
        let Some(entry) = found.db else {
            return Ok(FolderCopyInfo {
                folder_copy: None,
                has_local,
                is_own: false,
            });
        };
        let fingerprint = fingerprint(&entry, found.wal.as_ref());
        let is_own = mirror::is_own(&base, fingerprint);
        Ok(FolderCopyInfo {
            folder_copy: Some(FolderCopyStat { fingerprint }),
            has_local,
            is_own,
        })
    }

    pub fn accept(tree: &str, fingerprint: mirror::Fingerprint, app: &AppHandle) -> AppResult<()> {
        let base = project_data_base(app, tree)?;
        std::fs::create_dir_all(&base)?;
        mirror::accept(&base, fingerprint)
    }

    pub fn import(tree: &str, app: &AppHandle, state: &AppState) -> AppResult<()> {
        let base = project_data_base(app, tree)?;
        let found = find(app, tree)?;
        let entry = found
            .db
            .ok_or_else(|| AppError::Other("This folder has no folder copy.".into()))?;
        let is_open = state
            .project
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|p| p.root == base);
        if is_open {
            close_project_inner(state);
        }
        // A scan or a command can still hold the old database after the close.
        // Replacing its files under that handle would corrupt both databases.
        let live = base.join(".cullant").join("cullant.db");
        if !Db::wait_closed_at(&live, std::time::Duration::from_secs(5)) {
            return Err(AppError::Other(
                "Cullant is still closing this project. Try again.".into(),
            ));
        }
        let staging = base.join("import.tmp");
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        let copy = staging.join("cullant.db");
        let result = (|| {
            stage(app, tree, &entry, found.wal.as_ref(), &staging)?;
            mirror::import_files(&base, &copy, fingerprint(&entry, found.wal.as_ref()))
        })();
        let _ = std::fs::remove_dir_all(&staging);
        result
    }
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
