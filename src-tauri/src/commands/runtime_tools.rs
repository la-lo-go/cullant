use serde::Serialize;
use tauri::State;

use crate::decode::{heif, video};
use crate::error::{AppError, AppResult};
use crate::AppState;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeToolStatus {
    platform: &'static str,
    ffmpeg_present: bool,
    ffmpeg_major: Option<u32>,
    ffprobe_present: bool,
    heif_available: bool,
    has_heif: bool,
    has_videos: bool,
    has_hevc: bool,
}

#[tauri::command]
pub async fn runtime_tools(state: State<'_, AppState>) -> AppResult<RuntimeToolStatus> {
    let (db, store) = {
        let guard = state.project.lock().unwrap();
        let project = guard.as_ref().ok_or(AppError::NoProject)?;
        (project.db.clone(), project.store.clone())
    };
    tauri::async_runtime::spawn_blocking(move || {
        let (has_heif, has_videos, has_hevc) = db.call_read(|conn| {
            Ok(conn.query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM files WHERE status=0 AND ext IN ({})),
                            EXISTS(SELECT 1 FROM files WHERE status=0 AND kind=2),
                            EXISTS(SELECT 1 FROM files WHERE status=0 AND kind=2 AND video_codec='hevc')",
                    crate::db::sql::heif_exts()
                ),
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )?)
        })?;
        // Android uses the platform media APIs. It cannot install desktop binaries.
        let desktop = !cfg!(target_os = "android");
        let ffmpeg = (desktop && (has_videos || has_heif)).then(video::ffmpeg_info);
        Ok(RuntimeToolStatus {
            platform: std::env::consts::OS,
            ffmpeg_present: ffmpeg.is_some_and(|info| info.present),
            ffmpeg_major: ffmpeg.and_then(|info| info.major),
            ffprobe_present: desktop && has_videos && video::ffprobe_available(),
            heif_available: !has_heif || heif::available(store.as_ref()),
            has_heif,
            has_videos,
            has_hevc,
        })
    })
    .await
    .map_err(|error| AppError::Other(error.to_string()))?
}
