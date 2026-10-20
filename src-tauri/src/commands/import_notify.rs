use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::{AppHandle, EventId, Listener, Manager};
use tauri_plugin_import_notify::{ImportNotifyExt, Outcome, Phase};

use crate::thumbs::ThumbPool;
use crate::AppState;

#[derive(Default)]
struct Seen {
    file_count: Option<u64>,
    empty: bool,
    error: Option<String>,
}

/// Keeps the Android import service alive while the scanner thread runs. It
/// reads the progress events the scan already emits, so the scan code does not
/// know about notifications. Dropping it ends the service, also after a panic.
pub struct ImportNotifyGuard {
    app: AppHandle,
    thumbs: Arc<ThumbPool>,
    generation: u64,
    name: String,
    seen: Arc<Mutex<Seen>>,
    listeners: Vec<(&'static str, EventId)>,
}

impl ImportNotifyGuard {
    /// `roots` are the ids the scan events carry as `projectRoot`.
    pub fn start(
        app: &AppHandle,
        thumbs: Arc<ThumbPool>,
        generation: u64,
        name: String,
        roots: [String; 2],
    ) -> Self {
        if let Err(error) = app.import_notify().start(&name) {
            tracing::debug!("import notification not started: {error}");
        }
        let seen = Arc::new(Mutex::new(Seen::default()));
        let mut listeners = Vec::new();
        for (event, phase) in [
            ("scan:progress", Phase::Finding),
            ("metadata:progress", Phase::Metadata),
            ("thumbs:progress", Phase::Thumbnails),
            ("previews:progress", Phase::Previews),
            ("videos:progress", Phase::Videos),
            ("scan:done", Phase::Finding),
            ("scan:error", Phase::Finding),
            ("scan:empty", Phase::Finding),
        ] {
            let (app_handle, seen, roots) = (app.clone(), seen.clone(), roots.clone());
            let id = app.listen(event, move |e| {
                let Ok(payload) = serde_json::from_str::<Value>(e.payload()) else {
                    return;
                };
                if !payload["projectRoot"]
                    .as_str()
                    .is_some_and(|r| roots.iter().any(|x| x == r))
                {
                    return;
                }
                let count = |key: &str| payload[key].as_u64().unwrap_or(0);
                let update = match event {
                    "scan:progress" => Some((count("found"), None)),
                    "scan:done" => {
                        seen.lock().unwrap().file_count = Some(count("fileCount"));
                        None
                    }
                    "scan:empty" => {
                        seen.lock().unwrap().empty = true;
                        None
                    }
                    "scan:error" => {
                        let message = payload["error"].as_str().unwrap_or_default();
                        seen.lock().unwrap().error = Some(message.to_string());
                        None
                    }
                    _ => Some((count("done"), Some(count("total")))),
                };
                if let Some((done, total)) = update {
                    let _ = app_handle.import_notify().update(phase, done, total);
                }
            });
            listeners.push((event, id));
        }
        Self {
            app: app.clone(),
            thumbs,
            generation,
            name,
            seen,
            listeners,
        }
    }

    fn end(&self) -> tauri_plugin_import_notify::Result<()> {
        let notify = self.app.import_notify();
        let seen = self.seen.lock().unwrap_or_else(|e| e.into_inner());
        // A rolled-back empty project is closed on purpose, so `empty` comes before the closed check.
        if seen.empty {
            return notify.finish(Outcome::Empty, &self.name, 0);
        }
        if self.project_replaced() {
            return notify.stop();
        }
        match (&seen.error, seen.file_count) {
            (Some(message), _) => notify.finish(Outcome::Error(message.clone()), &self.name, 0),
            (None, Some(count)) => notify.finish(Outcome::Done, &self.name, count),
            (None, None) => notify.stop(),
        }
    }

    fn project_replaced(&self) -> bool {
        let state = self.app.state::<AppState>();
        let guard = state.project.lock().unwrap();
        self.thumbs.is_shut_down()
            || guard
                .as_ref()
                .is_none_or(|p| p.db.generation() != self.generation)
    }
}

impl Drop for ImportNotifyGuard {
    fn drop(&mut self) {
        for (_, id) in &self.listeners {
            self.app.unlisten(*id);
        }
        if let Err(error) = self.end() {
            tracing::debug!("import notification not ended: {error}");
        }
    }
}
