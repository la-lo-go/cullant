use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::error::Result;
use crate::models::{Outcome, Phase};

/// Desktop stub: an import there needs no foreground service, so every call
/// succeeds and does nothing.
pub struct ImportNotify<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<ImportNotify<R>> {
    Ok(ImportNotify(app.clone()))
}

impl<R: Runtime> ImportNotify<R> {
    pub fn start(&self, _project_name: &str) -> Result<()> {
        Ok(())
    }
    pub fn update(&self, _phase: Phase, _done: u64, _total: Option<u64>) -> Result<()> {
        Ok(())
    }
    pub fn finish(&self, _outcome: Outcome, _project_name: &str, _file_count: u64) -> Result<()> {
        Ok(())
    }
    pub fn stop(&self) -> Result<()> {
        Ok(())
    }
    pub fn request_permission(&self) -> Result<bool> {
        Ok(true)
    }
    pub fn open_settings(&self) -> Result<()> {
        Ok(())
    }
}
