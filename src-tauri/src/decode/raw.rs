use std::path::Path;

use image::DynamicImage;
use rawler::decoders::RawDecodeParams;
use rawler::rawsource::RawSource;

use crate::error::{AppError, AppResult};

/// Extract the camera-embedded preview JPEG from a RAW file — the fast path
/// for culling: no demosaic, just pull the largest preview the container has.
pub fn embedded_preview(path: &Path) -> AppResult<DynamicImage> {
    let source =
        RawSource::new(path).map_err(|e| AppError::Decode(format!("{}: {e}", path.display())))?;
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
            Err(e) => tracing::debug!("preview extraction step failed for {path:?}: {e}"),
        }
    }
    Err(AppError::Decode(format!(
        "no embedded preview in {}",
        path.display()
    )))
}

pub struct RawMeta {
    pub capture_time: Option<i64>,
    pub orientation: Option<u16>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
}

pub fn read_metadata(path: &Path) -> AppResult<RawMeta> {
    let source =
        RawSource::new(path).map_err(|e| AppError::Decode(format!("{}: {e}", path.display())))?;
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
