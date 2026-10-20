use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
use tauri::{AppHandle, EventId, Listener, Manager};
use tauri_plugin_import_notify::{ImportNotifyExt, Outcome, Phase};

use crate::commands::import_progress::{Expect, Tracker};
use crate::db::Db;
use crate::error::AppResult;

/// Token of the guard that owns the single Android service. A guard from an
/// earlier open must not end the service of a newer one.
static OWNER: Mutex<u64> = Mutex::new(0);

/// Guards that exist now. An early guard of an open that then fails would end the
/// service of an older import that still runs, so an open starts early only when this is 0.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// What a project holds. A photo is a shot: a RAW+JPEG pair counts once, but the
/// members of a decoupled group count each, like the grid and the preview pass do.
#[derive(Clone, Copy, Default)]
pub struct Counts {
    pub photos: u64,
    pub videos: u64,
}

impl Counts {
    pub fn load(db: &Db) -> AppResult<Self> {
        db.call_read(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(DISTINCT CASE WHEN f.kind IN (0, 1) AND g.decoupled = 0 THEN f.group_id END)
                        + COUNT(CASE WHEN f.kind IN (0, 1) AND g.decoupled = 1 THEN 1 END),
                        COUNT(CASE WHEN f.kind = 2 THEN 1 END)
                 FROM files f JOIN groups g ON g.id = f.group_id
                 WHERE f.status = 0",
                [],
                |r| {
                    Ok(Self {
                        photos: r.get::<_, i64>(0)? as u64,
                        videos: r.get::<_, i64>(1)? as u64,
                    })
                },
            )?)
        })
    }
}

/// How the scanner thread ended. `None` instead of an `End` means it was cancelled or replaced.
pub enum End {
    Done(Counts),
    Empty,
    Error(String),
}

/// Keeps the Android import service alive while the scanner thread runs.
/// Dropping it ends the service, also after a panic.
pub struct ImportNotifyGuard {
    app: AppHandle,
    name: String,
    token: u64,
    end: Option<End>,
    listeners: Vec<EventId>,
    tracker: Arc<Mutex<Tracker>>,
}

impl ImportNotifyGuard {
    pub fn any_alive() -> bool {
        LIVE.load(Ordering::Acquire) > 0
    }

    /// `root` is the id the scan events carry as `projectRoot`.
    pub fn start(app: &AppHandle, name: String, root: String) -> Self {
        let mut owner = OWNER.lock().unwrap_or_else(|e| e.into_inner());
        *owner += 1;
        LIVE.fetch_add(1, Ordering::AcqRel);
        if let Err(error) = app.import_notify().start(&name) {
            tracing::debug!("import notification not started: {error}");
        }
        // Progress is best effort: an event may arrive late or not at all. The
        // outcome never comes from events, the scanner thread hands it over.
        // Thumbnail progress before the first preview is the short lead pass,
        // whose total is not the library size.
        let previews_started = Arc::new(AtomicBool::new(false));
        let tracker = Arc::new(Mutex::new(Tracker::default()));
        let listeners = [
            ("scan:progress", Phase::Finding),
            ("metadata:progress", Phase::Metadata),
            ("thumbs:progress", Phase::Thumbnails),
            ("previews:progress", Phase::Previews),
            ("videos:progress", Phase::Videos),
        ]
        .map(|(event, phase)| {
            let (app_handle, root) = (app.clone(), root.clone());
            let previews_started = previews_started.clone();
            let tracker = tracker.clone();
            app.listen(event, move |e| {
                let Ok(payload) = serde_json::from_str::<Value>(e.payload()) else {
                    return;
                };
                if !payload["projectRoot"].as_str().is_some_and(|r| r == root) {
                    return;
                }
                let count = |key: &str| payload[key].as_u64().unwrap_or(0);
                let phase = match phase {
                    Phase::Previews => {
                        previews_started.store(true, Ordering::Relaxed);
                        phase
                    }
                    Phase::Thumbnails if !previews_started.load(Ordering::Relaxed) => {
                        Phase::FirstThumbnails
                    }
                    _ => phase,
                };
                let (done, total) = if phase == Phase::Finding {
                    (count("found"), None)
                } else {
                    (count("done"), Some(count("total")))
                };
                // Held across the send, so two events cannot reach the worker out of order.
                let mut tracker = tracker.lock().unwrap_or_else(|e| e.into_inner());
                let overall = tracker.update(phase, done, total, Instant::now());
                let _ = app_handle
                    .import_notify()
                    .update(phase, done, total, overall);
            })
        })
        .to_vec();
        Self {
            app: app.clone(),
            name,
            token: *owner,
            // The default covers a scanner thread that never starts. A started one replaces it.
            end: Some(End::Error("Could not start the import.".into())),
            listeners,
            tracker,
        }
    }

    /// Gives the bar the size of the phases that have not started. Call it when the walk ends.
    pub fn expect(&self, counts: Counts) {
        let generate_videos = self
            .app
            .state::<crate::AppState>()
            .generate_video_thumbs
            .load(Ordering::Relaxed);
        let expect = Expect {
            photos: counts.photos,
            videos: if generate_videos { counts.videos } else { 0 },
        };
        self.tracker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .expect(expect);
    }

    pub fn set_end(&mut self, end: Option<End>) {
        self.end = end;
    }

    fn send_end(&mut self) {
        let notify = self.app.import_notify();
        let result = match self.end.take() {
            Some(End::Done(counts)) => notify.finish(
                Outcome::Done {
                    photos: counts.photos,
                    videos: counts.videos,
                },
                &self.name,
            ),
            Some(End::Empty) => notify.finish(Outcome::Empty, &self.name),
            Some(End::Error(message)) => notify.finish(Outcome::Error(message), &self.name),
            None => notify.stop(),
        };
        if let Err(error) = result {
            tracing::debug!("import notification not ended: {error}");
        }
    }
}

impl Drop for ImportNotifyGuard {
    fn drop(&mut self) {
        for id in &self.listeners {
            self.app.unlisten(*id);
        }
        let owner = OWNER.lock().unwrap_or_else(|e| e.into_inner());
        if *owner == self.token {
            self.send_end();
        }
        LIVE.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decoupled_pair_counts_as_two_photos() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.jpg", "a.cr3", "b.jpg", "b.cr3", "c.mp4"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let db = Arc::new(Db::open(dir.path()).unwrap());
        crate::scan::scan_project_inner(&db, dir.path(), &mut |_| {}).unwrap();
        db.call(|conn| {
            conn.execute(
                "UPDATE groups SET decoupled = 1
                 WHERE id IN (SELECT group_id FROM files WHERE rel_path = 'b.jpg')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let counts = Counts::load(&db).unwrap();
        assert_eq!((counts.photos, counts.videos), (3, 1));
    }
}
