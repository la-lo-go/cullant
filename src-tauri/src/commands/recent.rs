use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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
const MAX_PROBES: usize = 4;
const PROBE_BUDGET: Duration = Duration::from_secs(1);
static PROBE_SLOTS: OnceLock<Arc<AtomicUsize>> = OnceLock::new();

struct ProbeSlot(Arc<AtomicUsize>);

impl ProbeSlot {
    fn take(slots: &Arc<AtomicUsize>) -> Option<Self> {
        slots
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < MAX_PROBES).then_some(count + 1)
            })
            .ok()
            .map(|_| Self(slots.clone()))
    }
}

impl Drop for ProbeSlot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

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
/// recent list (or inserting it). Errors are swallowed so a broken app-data
/// directory never blocks opening a project.
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
#[tauri::command(async)]
pub fn list_recent_projects(app: AppHandle) -> Vec<RecentProject> {
    let entries = load_recent(&app);
    let slots = PROBE_SLOTS
        .get_or_init(|| Arc::new(AtomicUsize::new(0)))
        .clone();
    probe_recent_projects(entries, slots, PROBE_BUDGET, move |path| {
        crate::storage::probe(&app, path)
    })
}

fn probe_recent_projects(
    mut entries: Vec<RecentEntry>,
    slots: Arc<AtomicUsize>,
    budget: Duration,
    probe: impl Fn(&str) -> crate::storage::StorageInfo + Send + Sync + 'static,
) -> Vec<RecentProject> {
    entries.truncate(MAX_RECENT);
    let deadline = Instant::now() + budget;
    let probe = Arc::new(probe);
    let (send, receive) = mpsc::channel();
    let mut results = vec![None; entries.len()];
    let mut next = 0;
    let mut outstanding = 0;
    loop {
        while next < entries.len() && Instant::now() < deadline {
            let Some(slot) = ProbeSlot::take(&slots) else {
                break;
            };
            let index = next;
            let path = entries[index].path.clone();
            let probe = probe.clone();
            let send = send.clone();
            let spawned = std::thread::Builder::new()
                .name("cullant-storage-probe".into())
                .spawn(move || {
                    let result = probe(&path);
                    // A timed-out provider keeps its permit until it returns.
                    drop(slot);
                    let _ = send.send((index, result));
                });
            next += 1;
            if spawned.is_ok() {
                outstanding += 1;
            }
        }
        if outstanding == 0 {
            break;
        }
        match receive.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((index, result)) => {
                results[index] = Some(result);
                outstanding -= 1;
            }
            Err(_) => break,
        }
    }
    entries
        .into_iter()
        .zip(results)
        .map(|(e, result)| {
            let mut storage = result.unwrap_or(crate::storage::StorageInfo {
                state: crate::storage::StorageState::Unknown,
                kind: crate::storage::StorageKind::Unknown,
                volume_name: None,
            });
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

/// Probe the storage backing an arbitrary project identifier so the UI can warn
/// when its folder or volume is disconnected or removed.
///
/// `async` puts it on the sync threadpool instead of running inline on the IPC
/// handler: on Android the probe makes two blocking JNI calls, and every other
/// command would otherwise queue behind them on a timer, forever.
#[tauri::command(async)]
pub fn probe_storage(app: AppHandle, id: String) -> crate::storage::StorageInfo {
    crate::storage::probe(&app, &id)
}

/// Fully forget a project: drop it from the recent list and delete Cullant's
/// own data (the SQLite DB + thumbnail cache). The user's photos are NEVER
/// touched — only Cullant's sidecar is removed.
///
/// Desktop: the sidecar is `<project>/.cullant`; it is removed permanently
/// (the DB + thumbnail cache regenerate on the next scan).
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

/// Remove Cullant's `.cullant` sidecar directory (just the SQLite DB and
/// thumbnail cache — the user's photos are never here). Removed permanently;
/// it is fully regenerated on the next scan.
fn delete_sidecar(dir: &std::path::Path) -> AppResult<()> {
    std::fs::remove_dir_all(dir).map_err(|e| AppError::Other(format!("delete project data: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{StorageInfo, StorageKind, StorageState};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Condvar, Mutex};
    use std::time::{Duration, Instant};

    fn entries() -> Vec<RecentEntry> {
        (0..24)
            .map(|i| RecentEntry {
                path: format!("fake-provider/project-{i}"),
                last_opened: i,
                kind: Some(StorageKind::Network),
                volume_name: Some("Remembered share".into()),
            })
            .collect()
    }

    #[test]
    fn recent_projects_return_ready_provider_results_within_one_budget() {
        let started = Instant::now();
        let projects = probe_recent_projects(
            entries(),
            Arc::new(AtomicUsize::new(0)),
            Duration::from_millis(100),
            |_| StorageInfo {
                state: StorageState::Ok,
                kind: StorageKind::Network,
                volume_name: Some("Live share".into()),
            },
        );
        assert_eq!(projects.len(), 24);
        assert!(projects
            .iter()
            .all(|p| p.available && p.storage.state == StorageState::Ok));
        assert!(projects
            .iter()
            .all(|p| p.storage.volume_name.as_deref() == Some("Live share")));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn stalled_provider_keeps_four_slots_until_it_finishes_and_returns_unknown() {
        let slots = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let started_count = Arc::new(AtomicUsize::new(0));
        let provider_gate = gate.clone();
        let provider_started = started_count.clone();
        let started = Instant::now();
        let first = probe_recent_projects(
            entries(),
            slots.clone(),
            Duration::from_millis(100),
            move |_| {
                provider_started.fetch_add(1, Ordering::SeqCst);
                let (lock, ready) = &*provider_gate;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
                StorageInfo {
                    state: StorageState::Ok,
                    kind: StorageKind::Network,
                    volume_name: None,
                }
            },
        );
        let first_elapsed = started.elapsed();
        let second =
            probe_recent_projects(entries(), slots.clone(), Duration::from_millis(100), |_| {
                panic!("A timed-out provider still owns its slot")
            });
        let retained = slots.load(Ordering::SeqCst);
        let attempts = started_count.load(Ordering::SeqCst);
        let (lock, ready) = &*gate;
        *lock.lock().unwrap() = true;
        ready.notify_all();
        let finish_by = Instant::now() + Duration::from_secs(1);
        while slots.load(Ordering::SeqCst) != 0 && Instant::now() < finish_by {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(first_elapsed < Duration::from_millis(500));
        assert_eq!(retained, 4);
        assert_eq!(attempts, 4);
        assert_eq!(slots.load(Ordering::SeqCst), 0);
        for project in first.iter().chain(&second) {
            assert!(!project.available);
            assert!(project.storage.state == StorageState::Unknown);
            assert!(project.storage.kind == StorageKind::Network);
            assert_eq!(
                project.storage.volume_name.as_deref(),
                Some("Remembered share")
            );
        }
    }
}
