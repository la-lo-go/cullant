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
/// The notification expires on its own (see `ImportNotifier.PROGRESS_TIMEOUT_MS`), so a
/// quiet phase must repeat the latest state well before that.
const HEARTBEAT: Duration = Duration::from_secs(60);
/// A time estimate older than this is stale: a heartbeat repeats the count only.
const ETA_FRESH: Duration = Duration::from_secs(30);

enum Msg {
    Start(String),
    Update(Phase, u64, Option<u64>, Overall),
    Finish(Outcome, String),
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

/// Sends updates to Android and remembers the last one for the heartbeat.
struct UpdateSender<'a, R: Runtime> {
    handle: &'a PluginHandle<R>,
    last_sent: Instant,
    /// When `latest` came from a real update and not from a heartbeat.
    last_real: Instant,
    latest: Option<UpdatePayload>,
}

impl<R: Runtime> UpdateSender<'_, R> {
    fn send(&mut self, payload: UpdatePayload) {
        self.last_real = Instant::now();
        self.deliver(payload);
    }

    fn heartbeat(&mut self) {
        let Some(mut payload) = self.latest.clone() else {
            return;
        };
        if self.last_real.elapsed() >= ETA_FRESH {
            payload.overall.eta_minutes = None;
        }
        self.deliver(payload);
    }

    fn deliver(&mut self, payload: UpdatePayload) {
        self.last_sent = Instant::now();
        self.latest = Some(payload.clone());
        let _ = self.handle.run_mobile_plugin::<Ack>("update", payload);
    }
}

/// Sends the queued messages to Android. Updates of one phase collapse to the
/// latest one, at most one per `UPDATE_INTERVAL`. A phase change, `Finish` and
/// `Start` flush the pending update first, so the last count is never lost.
/// While an import runs, the latest update repeats every `HEARTBEAT`.
/// Errors are ignored: Kotlin logs its own failures and an import must not care.
fn run_worker<R: Runtime>(handle: &PluginHandle<R>, rx: Receiver<Msg>) {
    let mut pending: Option<UpdatePayload> = None;
    let mut sent_phase: Option<Phase> = None;
    let mut sender = UpdateSender {
        handle,
        last_sent: Instant::now(),
        last_real: Instant::now(),
        latest: None,
    };

    loop {
        let wait = match (&pending, &sender.latest) {
            (Some(_), _) => Some(UPDATE_INTERVAL),
            (None, Some(_)) => Some(HEARTBEAT),
            (None, None) => None,
        };
        let msg = match wait {
            None => rx.recv().ok(),
            Some(wait) => match rx.recv_timeout(wait.saturating_sub(sender.last_sent.elapsed())) {
                Ok(msg) => Some(msg),
                Err(RecvTimeoutError::Timeout) => {
                    match pending.take() {
                        Some(payload) => sender.send(payload),
                        None => sender.heartbeat(),
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => None,
            },
        };
        let Some(msg) = msg else { return };

        match msg {
            Msg::Update(phase, done, total, overall) => {
                let payload = UpdatePayload {
                    phase,
                    done,
                    total,
                    overall,
                };
                if sent_phase == Some(phase) {
                    // Without this, a flood of updates keeps replacing the pending one and none is sent.
                    if sender.last_sent.elapsed() >= UPDATE_INTERVAL {
                        pending = None;
                        sender.send(payload);
                    } else {
                        pending = Some(payload);
                    }
                } else {
                    if let Some(old) = pending.take() {
                        sender.send(old);
                    }
                    sent_phase = Some(phase);
                    sender.send(payload);
                }
            }
            Msg::Start(project_name) => {
                pending = None;
                sent_phase = None;
                sender.latest = None;
                let _ = handle.run_mobile_plugin::<Ack>(
                    "start",
                    StartPayload {
                        project_name: &project_name,
                    },
                );
            }
            Msg::Finish(outcome, project_name) => {
                if let Some(payload) = pending.take() {
                    sender.send(payload);
                }
                sent_phase = None;
                sender.latest = None;
                let (outcome, error, photos, videos) = match &outcome {
                    Outcome::Done { photos, videos } => ("done", None, *photos, *videos),
                    Outcome::Empty => ("empty", None, 0, 0),
                    Outcome::Error(message) => ("error", Some(message.as_str()), 0, 0),
                };
                let _ = handle.run_mobile_plugin::<Ack>(
                    "finish",
                    FinishPayload {
                        outcome,
                        error,
                        project_name: &project_name,
                        photos,
                        videos,
                    },
                );
            }
            Msg::Stop => {
                // The notification goes away, so a pending count has no reader.
                pending = None;
                sent_phase = None;
                sender.latest = None;
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

    /// Update the progress notification. `done` and `total` count the current
    /// phase and show as text. The bar shows `overall`.
    /// Returns at once; updates are coalesced to at most two per second.
    pub fn update(
        &self,
        phase: Phase,
        done: u64,
        total: Option<u64>,
        overall: Overall,
    ) -> Result<()> {
        self.queue(Msg::Update(phase, done, total, overall))
    }

    /// Stop the service. Post a result notification only if the app is in the background.
    pub fn finish(&self, outcome: Outcome, project_name: &str) -> Result<()> {
        self.queue(Msg::Finish(outcome, project_name.to_string()))
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
