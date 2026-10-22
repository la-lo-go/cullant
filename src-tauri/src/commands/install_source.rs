use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_saf::SafExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSource {
    os: &'static str,
    installer: Option<String>,
}

/// The frontend maps the installer to an update page. A failed lookup is the
/// same as an unknown installer, so it never blocks the caller.
#[tauri::command]
pub fn install_source(app: AppHandle) -> InstallSource {
    InstallSource {
        os: std::env::consts::OS,
        installer: app.saf().installer_package().ok().flatten(),
    }
}
