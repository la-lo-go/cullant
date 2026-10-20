use std::sync::Mutex;

use serde_json::Value;
use tauri::{AppHandle, EventId, Listener};
use tauri_plugin_import_notify::{ImportNotifyExt, Outcome, Phase};

/// Token of the guard that owns the single Android service. A guard from an
/// earlier open must not end the service of a newer one.
static OWNER: Mutex<u64> = Mutex::new(0);

/// How the scanner thread ended. `None` instead of an `End` means it was cancelled or replaced.
pub enum End {
    Done(u64),
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
}

impl ImportNotifyGuard {
    /// `roots` are the ids the scan events carry as `projectRoot`.
    pub fn start(app: &AppHandle, name: String, roots: [String; 2]) -> Self {
        let mut owner = OWNER.lock().unwrap_or_else(|e| e.into_inner());
        *owner += 1;
        if let Err(error) = app.import_notify().start(&name) {
            tracing::debug!("import notification not started: {error}");
        }
        // Progress is best effort: an event may arrive late or not at all. The
        // outcome never comes from events, the scanner thread hands it over.
        let listeners = [
            ("scan:progress", Phase::Finding),
            ("metadata:progress", Phase::Metadata),
            ("thumbs:progress", Phase::Thumbnails),
            ("previews:progress", Phase::Previews),
            ("videos:progress", Phase::Videos),
        ]
        .map(|(event, phase)| {
            let (app_handle, roots) = (app.clone(), roots.clone());
            app.listen(event, move |e| {
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
                let (done, total) = if phase == Phase::Finding {
                    (count("found"), None)
                } else {
                    (count("done"), Some(count("total")))
                };
                let _ = app_handle.import_notify().update(phase, done, total);
            })
        })
        .to_vec();
        Self {
            app: app.clone(),
            name,
            token: *owner,
            end: None,
            listeners,
        }
    }

    pub fn set_end(&mut self, end: Option<End>) {
        self.end = end;
    }

    fn send_end(&mut self) {
        let notify = self.app.import_notify();
        let result = match self.end.take() {
            Some(End::Done(count)) => notify.finish(Outcome::Done, &self.name, count),
            Some(End::Empty) => notify.finish(Outcome::Empty, &self.name, 0),
            Some(End::Error(message)) => notify.finish(Outcome::Error(message), &self.name, 0),
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
    }
}
