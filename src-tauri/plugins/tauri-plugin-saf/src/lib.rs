//! Storage Access Framework (SAF) bridge for Android.
//!
//! Exposes Android `DocumentsContract`/`ContentResolver` tree operations to
//! Rust so Cullant can list/read/write/rename/move/copy/delete files inside a
//! user-picked SD card or USB volume (`content://` tree URI) — none of which is
//! reachable through `std::fs`. All commands are invoked Rust-side via
//! `run_mobile_plugin`; nothing is exposed to JS.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

mod error;
// The request/response payloads are only consumed by the Android (`mobile`)
// module; on desktop they are intentionally unused.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
mod models;

pub use error::{Error, Result};
pub use models::{SafEntry, VideoMetadata, VolumeInfo};

#[cfg(target_os = "android")]
mod mobile;
#[cfg(target_os = "android")]
pub use mobile::Saf;

#[cfg(not(target_os = "android"))]
mod desktop;
#[cfg(not(target_os = "android"))]
pub use desktop::Saf;

/// Access the SAF handle from any Tauri `Manager` (app handle, window, state).
pub trait SafExt<R: Runtime> {
    fn saf(&self) -> &Saf<R>;
}

impl<R: Runtime, T: Manager<R>> SafExt<R> for T {
    fn saf(&self) -> &Saf<R> {
        self.state::<Saf<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("saf")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            let saf = mobile::init(app, api)?;
            #[cfg(not(target_os = "android"))]
            let saf = desktop::init(app, api)?;
            app.manage(saf);
            Ok(())
        })
        .build()
}
