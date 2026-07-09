use std::sync::Arc;

use image::DynamicImage;
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;

use crate::error::{AppError, AppResult};

/// Extract the camera-embedded preview JPEG from RAW bytes — the fast path
/// for culling: no demosaic, just pull the largest preview the container has.
/// `name` is only used for error/log messages (a rel_path). Takes owned bytes
/// so it works over any storage backend (real file or SAF stream), not just an
/// mmap'able path.
pub fn embedded_preview(bytes: Arc<Vec<u8>>, name: &str) -> AppResult<DynamicImage> {
    let source = RawSource::new_from_shared_vec(bytes).with_path(name);
    let decoder = rawler::get_decoder(&source).map_err(|e| AppError::Decode(format!("{e}")))?;
    let params = RawDecodeParams::default();

    for attempt in [
        decoder.full_image(&source, &params),
        decoder.preview_image(&source, &params),
        decoder.thumbnail_image(&source, &params),
    ] {
        match attempt {
            Ok(Some(img)) => return Ok(img),
            Ok(None) => continue,
            Err(e) => tracing::debug!("preview extraction step failed for {name}: {e}"),
        }
    }
    Err(AppError::Decode(format!("no embedded preview in {name}")))
}

pub struct RawMeta {
    pub capture_time: Option<i64>,
    pub orientation: Option<u16>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
}

pub fn read_metadata(bytes: Arc<Vec<u8>>, name: &str) -> AppResult<RawMeta> {
    let source = RawSource::new_from_shared_vec(bytes).with_path(name);
    let decoder = rawler::get_decoder(&source).map_err(|e| AppError::Decode(format!("{e}")))?;
    let md = decoder
        .raw_metadata(&source, &RawDecodeParams::default())
        .map_err(|e| AppError::Decode(format!("{e}")))?;

    let camera = match (md.make.trim(), md.model.trim()) {
        ("", "") => None,
        (make, model) => Some(format!("{make} {model}").trim().to_string()),
    };
    let iso = md
        .exif
        .iso_speed
        .or(md.exif.iso_speed_ratings.map(u32::from));

    Ok(RawMeta {
        capture_time: md
            .exif
            .date_time_original
            .as_deref()
            .or(md.exif.create_date.as_deref())
            .and_then(super::exif::parse_exif_datetime),
        orientation: md.exif.orientation,
        camera,
        lens: md.exif.lens_model.clone(),
        iso,
    })
}
