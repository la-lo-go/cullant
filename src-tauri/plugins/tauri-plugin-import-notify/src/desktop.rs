use serde::de::DeserializeOwned;
use std::marker::PhantomData;

use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::error::Result;
use crate::models::{Outcome, Overall, Phase};

/// Desktop stub: an import there needs no foreground service, so every call
/// succeeds and does nothing.
pub struct ImportNotify<R: Runtime>(PhantomData<fn() -> R>);

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<ImportNotify<R>> {
    Ok(ImportNotify(PhantomData))
}

impl<R: Runtime> ImportNotify<R> {
    pub fn start(&self, _project_name: &str) -> Result<()> {
        Ok(())
    }
    pub fn update(
        &self,
        _phase: Phase,
        _done: u64,
        _total: Option<u64>,
        _overall: Overall,
    ) -> Result<()> {
        Ok(())
    }
    pub fn finish(&self, _outcome: Outcome, _project_name: &str) -> Result<()> {
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
