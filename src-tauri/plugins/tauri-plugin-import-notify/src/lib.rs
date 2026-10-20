//! Foreground service and progress notification for project imports.
//!
//! Android stops a background process that holds no foreground service, which
//! kills a long import. This plugin keeps the process alive and shows progress.
//! On desktop every call succeeds and does nothing.
//!
//! Callers must treat every `Result` as best effort: a failed notification never
//! justifies a failed import.

use tauri::{
    plugin::{Builder, TauriPlugin},
    AppHandle, Manager, Runtime,
};

mod error;
// The request/response payloads are only consumed by the Android (`mobile`)
// module; on desktop they are intentionally unused.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod models;

pub use error::{Error, Result};
pub use models::{Outcome, Phase};

#[cfg(target_os = "android")]
mod mobile;
#[cfg(target_os = "android")]
pub use mobile::ImportNotify;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(not(target_os = "android"))]
pub use desktop::ImportNotify;

/// Access the import-notification handle from any Tauri `Manager`.
pub trait ImportNotifyExt<R: Runtime> {
    fn import_notify(&self) -> &ImportNotify<R>;
}

impl<R: Runtime, T: Manager<R>> ImportNotifyExt<R> for T {
    fn import_notify(&self) -> &ImportNotify<R> {
        self.state::<ImportNotify<R>>().inner()
    }
}

// Async on purpose: a sync command runs on the main thread, and the permission
// dialog needs that thread to deliver its answer.
#[tauri::command]
async fn request_permission<R: Runtime>(app: AppHandle<R>) -> Result<bool> {
    app.import_notify().request_permission()
}

#[tauri::command]
async fn open_settings<R: Runtime>(app: AppHandle<R>) -> Result<()> {
    app.import_notify().open_settings()
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("import-notify")
        .invoke_handler(tauri::generate_handler![request_permission, open_settings])
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let handle = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let handle = desktop::init(app, api)?;
            app.manage(handle);
            Ok(())
        })
        .build()
}
