use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use tauri::plugin::PluginHandle;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::error::{Error, Result};
use crate::models::*;

const PLUGIN_IDENTIFIER: &str = "app.tauri.importnotify";
const UPDATE_INTERVAL: Duration = Duration::from_millis(500);

enum Msg {
    Start(String),
    Update(Phase, u64, Option<u64>),
    Finish(Outcome, String, u64),
    Stop,
}

/// Handle to the Android import-notification plugin.
///
/// `run_mobile_plugin` blocks until the Android main thread runs the command.
/// The progress calls therefore only queue a message for a worker thread, so a
/// caller (the scanner, or a thread that holds a lock) never waits on the UI.
pub struct ImportNotify<R: Runtime> {
    handle: Arc<PluginHandle<R>>,
    tx: Sender<Msg>,
}

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<ImportNotify<R>> {
    let handle = Arc::new(api.register_android_plugin(PLUGIN_IDENTIFIER, "ImportNotifyPlugin")?);
    let (tx, rx) = channel();
    let worker_handle = handle.clone();
    std::thread::spawn(move || run_worker(&worker_handle, rx));
    Ok(ImportNotify { handle, tx })
}

/// Sends the queued messages to Android. Updates of one phase collapse to the
/// latest one, at most one per `UPDATE_INTERVAL`. A phase change, `Finish` and
/// `Start` flush the pending update first, so the last count is never lost.
/// Errors are ignored: Kotlin logs its own failures and an import must not care.
fn run_worker<R: Runtime>(handle: &PluginHandle<R>, rx: Receiver<Msg>) {
    let mut pending: Option<UpdatePayload> = None;
    let mut sent_phase: Option<Phase> = None;
    let mut last_sent = Instant::now();

    let send_update = |payload: UpdatePayload, last_sent: &mut Instant| {
        *last_sent = Instant::now();
        let _ = handle.run_mobile_plugin::<Ack>("update", payload);
    };

    loop {
        let msg = if pending.is_none() {
            rx.recv().ok()
        } else {
            match rx.recv_timeout(UPDATE_INTERVAL.saturating_sub(last_sent.elapsed())) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(payload) = pending.take() {
                        send_update(payload, &mut last_sent);
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => None,
            }
        };
        let Some(msg) = msg else { return };

        match msg {
            Msg::Update(phase, done, total) => {
                let payload = UpdatePayload { phase, done, total };
                if sent_phase == Some(phase) {
                    pending = Some(payload);
                } else {
                    if let Some(old) = pending.take() {
                        send_update(old, &mut last_sent);
                    }
                    sent_phase = Some(phase);
                    send_update(payload, &mut last_sent);
                }
            }
            Msg::Start(project_name) => {
                pending = None;
                sent_phase = None;
                let _ = handle.run_mobile_plugin::<Ack>(
                    "start",
                    StartPayload {
                        project_name: &project_name,
                    },
                );
            }
            Msg::Finish(outcome, project_name, file_count) => {
                if let Some(payload) = pending.take() {
                    send_update(payload, &mut last_sent);
                }
                sent_phase = None;
                let (outcome, error) = match &outcome {
                    Outcome::Done => ("done", None),
                    Outcome::Empty => ("empty", None),
                    Outcome::Error(message) => ("error", Some(message.as_str())),
                };
                let _ = handle.run_mobile_plugin::<Ack>(
                    "finish",
                    FinishPayload {
                        outcome,
                        error,
                        project_name: &project_name,
                        file_count,
                    },
                );
            }
            Msg::Stop => {
                // The notification goes away, so a pending count has no reader.
                pending = None;
                sent_phase = None;
                let _ = handle.run_mobile_plugin::<Ack>("stop", ());
            }
        }
    }
}

impl<R: Runtime> ImportNotify<R> {
    fn queue(&self, msg: Msg) -> Result<()> {
        self.tx.send(msg).map_err(|_| Error::WorkerStopped)
    }

    /// Start the foreground service with a "Preparing import" notification.
    /// Returns at once; the work happens on a worker thread.
    pub fn start(&self, project_name: &str) -> Result<()> {
        self.queue(Msg::Start(project_name.to_string()))
    }

    /// Update the progress notification. `total: None` shows an indeterminate bar.
    /// Returns at once; updates are coalesced to at most two per second.
    pub fn update(&self, phase: Phase, done: u64, total: Option<u64>) -> Result<()> {
        self.queue(Msg::Update(phase, done, total))
    }

    /// Stop the service. Post a result notification only if the app is in the background.
    pub fn finish(&self, outcome: Outcome, project_name: &str, file_count: u64) -> Result<()> {
        self.queue(Msg::Finish(outcome, project_name.to_string(), file_count))
    }

    /// Stop the service and post nothing.
    pub fn stop(&self) -> Result<()> {
        self.queue(Msg::Stop)
    }

    /// Ask for `POST_NOTIFICATIONS` (API 33+). Returns whether it is granted.
    /// Blocks until the user answers: call it from an async command only.
    pub fn request_permission(&self) -> Result<bool> {
        let res: GrantedResponse = self.handle.run_mobile_plugin("requestPermission", ())?;
        Ok(res.granted)
    }

    pub fn open_settings(&self) -> Result<()> {
        self.handle.run_mobile_plugin::<Ack>("openSettings", ())?;
        Ok(())
    }
}
