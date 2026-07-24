mod commands;
mod db;
mod decode;
mod engine;
mod error;
mod protocol;
mod scan;
mod storage;
mod store;
mod thumbs;

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use db::Db;
use store::ProjectStore;
use thumbs::ThumbPool;

/// State of the currently open project.
pub struct ProjectState {
    /// Real-filesystem base for this project's sidecar data (the `.cullant`
    /// dir): the project root itself on desktop, a private app dir on Android.
    pub root: PathBuf,
    /// Backend for the project's media (real fs on desktop, SAF on Android).
    pub store: Arc<dyn ProjectStore>,
    pub db: Arc<Db>,
    /// Shared with the scanner thread so ingest Phase B routes its bulk
    /// pregeneration through the same worker pool (background priority) instead
    /// of a second pool that would oversubscribe the CPU.
    pub thumbs: Arc<ThumbPool>,
}

pub struct AppState {
    pub project: Mutex<Option<ProjectState>>,
    /// Whether ingest pregenerates video poster thumbnails (they run last, after
    /// every photo thumbnail and preview, because ffmpeg poster extraction is the
    /// slowest tier). Pushed from the frontend setting; defaults to on. Only
    /// gates *background* pregeneration — a video's poster is still made on demand
    /// when its cell scrolls into view.
    pub generate_video_thumbs: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            project: Mutex::new(None),
            generate_video_thumbs: AtomicBool::new(true),
        }
    }
}

/// Thin facade for the `bench_ingest` example. Not a stable API.
#[doc(hidden)]
pub mod bench {
    use std::path::Path;
    use std::sync::Arc;

    pub use crate::db::Db;
    pub use crate::store::ProjectStore;

    pub fn open_db(root: &Path) -> Db {
        Db::open(root).expect("failed to open project db")
    }

    pub fn local_store(root: &Path) -> Arc<dyn ProjectStore> {
        Arc::new(crate::store::LocalFsStore::new(root))
    }

    /// Walk + reconcile; returns the number of present files.
    pub fn scan(db: &Arc<Db>, store: &dyn ProjectStore) -> i64 {
        crate::scan::scan_with_store(db, store, &mut |_| {})
            .expect("scan failed")
            .file_count
    }

    /// Run the two-phase ingest; returns
    /// `(metadata updated, thumbnails done, previews done)`.
    pub fn ingest(
        db: &Arc<Db>,
        store: &Arc<dyn ProjectStore>,
        root: &Path,
    ) -> (usize, usize, usize) {
        let thumbs = crate::thumbs::ThumbPool::start(db.clone(), store.clone(), root.to_path_buf());
        let mut meta = 0usize;
        let mut thumbs_done = 0usize;
        let mut previews = 0usize;
        crate::scan::ingest::run_ingest_inner(
            db,
            store.as_ref(),
            &thumbs,
            &|_, _| {},
            &mut |updated| meta = updated,
            &mut |done, _| thumbs_done = done,
            &mut |done, _| previews = done,
            &mut |_, _| {},
            true,
        )
        .expect("ingest failed");
        (meta, thumbs_done, previews)
    }

    /// Synthetic-photo EXIF payload for [`jpeg_with_exif`].
    pub struct SyntheticExif<'a> {
        /// `YYYY:MM:DD HH:MM:SS` (EXIF DateTimeOriginal format).
        pub date_time_original: &'a str,
        pub orientation: u16,
        pub make: &'a str,
        pub model: &'a str,
        pub iso: u16,
    }

    /// Encode an image as JPEG (q90) with a real EXIF APP1 block, so synthetic
    /// test data exercises the metadata-extraction path instead of the mtime
    /// fallback. Built with kamadak-exif's experimental writer (the same
    /// library the app reads with, so round-tripping is guaranteed): the
    /// writer emits a TIFF blob, which becomes `APP1 = "Exif\0\0" + TIFF`
    /// spliced right after the JPEG SOI marker.
    pub fn jpeg_with_exif(img: &image::RgbImage, meta: &SyntheticExif) -> Vec<u8> {
        use exif::experimental::Writer;
        use exif::{Field, In, Tag, Value};

        let mut jpeg = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90);
        img.write_with_encoder(enc).expect("jpeg encode failed");

        let fields = [
            Field {
                tag: Tag::DateTimeOriginal,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![meta.date_time_original.as_bytes().to_vec()]),
            },
            Field {
                tag: Tag::Orientation,
                ifd_num: In::PRIMARY,
                value: Value::Short(vec![meta.orientation]),
            },
            Field {
                tag: Tag::Make,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![meta.make.as_bytes().to_vec()]),
            },
            Field {
                tag: Tag::Model,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![meta.model.as_bytes().to_vec()]),
            },
            Field {
                tag: Tag::PhotographicSensitivity,
                ifd_num: In::PRIMARY,
                value: Value::Short(vec![meta.iso]),
            },
        ];
        let mut writer = Writer::new();
        for f in &fields {
            writer.push_field(f);
        }
        let mut tiff = std::io::Cursor::new(Vec::new());
        writer.write(&mut tiff, false).expect("exif write failed");
        let tiff = tiff.into_inner();

        // Splice APP1 right after SOI: FF E1 <len> "Exif\0\0" <tiff>.
        // <len> counts itself (2) plus the Exif header (6) plus the payload.
        let mut out = Vec::with_capacity(jpeg.len() + tiff.len() + 10);
        out.extend_from_slice(&jpeg[..2]);
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&((tiff.len() + 8) as u16).to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&jpeg[2..]);
        out
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().init();

    // Mobile: cap the decode pool. Phones report 8 cores but sustain far
    // fewer under thermal/memory pressure, and each in-flight decode holds
    // multi-MB buffers. Best-effort — a later init error just keeps defaults.
    #[cfg(target_os = "android")]
    {
        let threads = std::thread::available_parallelism()
            .map(|n| n.get().min(4))
            .unwrap_or(2);
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global();
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_saf::init())
        .manage(AppState::default())
        .register_asynchronous_uri_scheme_protocol("cullant", |ctx, request, responder| {
            protocol::handle(ctx.app_handle(), request, responder);
        })
        .setup(|app| {
            // The main window ships with `decorations: false` so Windows gets a
            // custom app-styled titlebar (see TitleBar.svelte). Custom chrome is
            // undesirable on macOS and uncertain on Linux, so restore the native
            // OS decorations there; the frontend likewise only renders the
            // custom bar on desktop Windows. Mobile has no window-decorations
            // concept at all (`set_decorations` isn't in WebviewWindow's API on
            // Android/iOS), so this is desktop-only, not just "not Windows".
            #[cfg(any(target_os = "macos", target_os = "linux"))]
            {
                use tauri::Manager;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_decorations(true);
                }
            }

            // Dev/test hook: auto-open a project folder at startup.
            if let Ok(path) = std::env::var("CULLANT_OPEN_PROJECT") {
                use tauri::Manager;
                let state = app.state::<AppState>();
                match commands::project::do_open_project(&path, app.handle(), &state) {
                    Ok(info) => tracing::info!("auto-opened project {}", info.root_path),
                    Err(e) => tracing::error!("CULLANT_OPEN_PROJECT failed: {e}"),
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::project::open_project,
            commands::project::pick_saf_tree,
            commands::project::current_project,
            commands::project::ingest_pending,
            commands::project::rescan_project,
            commands::project::close_project,
            commands::project::set_generate_video_thumbs,
            commands::recent::list_recent_projects,
            commands::recent::probe_storage,
            commands::recent::delete_project_data,
            commands::catalog::query_items,
            commands::catalog::media_counts,
            commands::catalog::preview_ready_ids,
            commands::culling::set_rating,
            commands::culling::set_flag,
            commands::culling::set_label,
            commands::culling::rotate,
            commands::groups::decouple_group,
            commands::groups::recouple_group,
            commands::tags::list_task_tags,
            commands::tags::create_task_tag,
            commands::tags::update_task_tag,
            commands::tags::delete_task_tag,
            commands::tags::toggle_task_tag,
            commands::tags::clear_task_tags,
            commands::actions::enqueue_action,
            commands::actions::remove_pending,
            commands::actions::remove_pending_for_files,
            commands::actions::clear_pending,
            commands::actions::list_pending,
            commands::actions::xmp_dirty_count,
            commands::actions::commit_preview,
            commands::actions::commit_execute,
            commands::actions::commit_execute_section,
            commands::actions::get_project_setting,
            commands::actions::set_project_setting,
            commands::session::get_session_state,
            commands::session::set_session_state,
            commands::metadata::get_file_metadata,
            commands::media::open_external,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod bench_tests {
    use super::bench::{jpeg_with_exif, SyntheticExif};

    /// The synthetic-EXIF writer must round-trip through the app's own reader.
    #[test]
    fn synthetic_exif_roundtrips() {
        let img = image::RgbImage::from_fn(120, 80, |x, _| image::Rgb([(x % 255) as u8, 40, 70]));
        let bytes = jpeg_with_exif(
            &img,
            &SyntheticExif {
                date_time_original: "2024:06:15 14:30:05",
                orientation: 6,
                make: "Canon",
                model: "EOS R5",
                iso: 400,
            },
        );
        let meta = crate::decode::exif::read_metadata(&bytes).unwrap();
        assert_eq!(meta.capture_time, Some(1718461805));
        assert_eq!(meta.orientation, Some(6));
        assert_eq!(meta.camera.as_deref(), Some("Canon EOS R5"));
        assert_eq!(meta.iso, Some(400));
        assert_eq!((meta.width, meta.height), (Some(120), Some(80)));
    }
}
