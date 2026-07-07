use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use time::macros::format_description;
use time::PrimitiveDateTime;

use crate::error::AppResult;

pub struct ImageMeta {
    pub capture_time: Option<i64>,
    pub orientation: Option<u16>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Parse EXIF `YYYY:MM:DD HH:MM:SS` into a unix timestamp. Times are treated
/// as UTC — culling only needs a stable sort order, not timezone math.
pub fn parse_exif_datetime(s: &str) -> Option<i64> {
    let fmt = format_description!("[year]:[month]:[day] [hour]:[minute]:[second]");
    PrimitiveDateTime::parse(s.trim(), &fmt)
        .ok()
        .map(|dt| dt.assume_utc().unix_timestamp())
}

/// Read EXIF + dimensions from a non-RAW image (JPEG/TIFF/PNG/WebP).
/// Everything is best-effort: a missing or corrupt EXIF block is not an error.
pub fn read_metadata(path: &Path) -> AppResult<ImageMeta> {
    let mut meta = ImageMeta {
        capture_time: None,
        orientation: None,
        camera: None,
        lens: None,
        iso: None,
        width: None,
        height: None,
    };

    if let Ok((w, h)) = image::image_dimensions(path) {
        meta.width = Some(w);
        meta.height = Some(h);
    }

    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut reader) else {
        return Ok(meta);
    };

    let field_str = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().to_string())
    };
    let field_uint = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
    };

    meta.capture_time = field_str(exif::Tag::DateTimeOriginal)
        .or_else(|| field_str(exif::Tag::DateTime))
        .and_then(|s| parse_exif_datetime(s.trim_matches('"')));
    meta.orientation = field_uint(exif::Tag::Orientation).map(|v| v as u16);
    meta.iso = field_uint(exif::Tag::PhotographicSensitivity);

    let clean = |s: String| {
        let s = s.trim().trim_matches('"').trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };
    let make = field_str(exif::Tag::Make).and_then(clean);
    let model = field_str(exif::Tag::Model).and_then(clean);
    meta.camera = match (make, model) {
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (m, None) => m,
        (None, m) => m,
    };
    meta.lens = field_str(exif::Tag::LensModel).and_then(clean);

    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_exif_datetime() {
        let ts = parse_exif_datetime("2024:06:15 14:30:05").unwrap();
        assert_eq!(ts, 1718461805);
    }

    #[test]
    fn rejects_garbage_datetime() {
        assert_eq!(parse_exif_datetime("not a date"), None);
    }
}
