use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::error::Result;
use crate::models::*;

const PLUGIN_IDENTIFIER: &str = "app.tauri.importnotify";

/// Handle to the Android import-notification plugin.
pub struct ImportNotify<R: Runtime>(tauri::plugin::PluginHandle<R>);

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<ImportNotify<R>> {
    let handle = api.register_android_plugin(PLUGIN_IDENTIFIER, "ImportNotifyPlugin")?;
    Ok(ImportNotify(handle))
}

impl<R: Runtime> ImportNotify<R> {
    /// Start the foreground service with a "Preparing import" notification.
    pub fn start(&self, project_name: &str) -> Result<()> {
        self.0
            .run_mobile_plugin::<Ack>("start", StartPayload { project_name })?;
        Ok(())
    }

    /// Update the progress notification. `total: None` shows an indeterminate bar.
    pub fn update(&self, phase: Phase, done: u64, total: Option<u64>) -> Result<()> {
        self.0
            .run_mobile_plugin::<Ack>("update", UpdatePayload { phase, done, total })?;
        Ok(())
    }

    /// Stop the service. Post a result notification only if the app is in the background.
    pub fn finish(&self, outcome: Outcome, project_name: &str, file_count: u64) -> Result<()> {
        let (outcome, error) = match &outcome {
            Outcome::Done => ("done", None),
            Outcome::Empty => ("empty", None),
            Outcome::Error(message) => ("error", Some(message.as_str())),
        };
        self.0.run_mobile_plugin::<Ack>(
            "finish",
            FinishPayload {
                outcome,
                error,
                project_name,
                file_count,
            },
        )?;
        Ok(())
    }

    /// Stop the service and post nothing.
    pub fn stop(&self) -> Result<()> {
        self.0.run_mobile_plugin::<Ack>("stop", ())?;
        Ok(())
    }

    /// Ask for `POST_NOTIFICATIONS` (API 33+). Returns whether it is granted.
    pub fn request_permission(&self) -> Result<bool> {
        let res: GrantedResponse = self.0.run_mobile_plugin("requestPermission", ())?;
        Ok(res.granted)
    }

    pub fn open_settings(&self) -> Result<()> {
        self.0.run_mobile_plugin::<Ack>("openSettings", ())?;
        Ok(())
    }
}
