use std::io::Cursor;

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

/// Read EXIF + dimensions from non-RAW image bytes (JPEG/TIFF/PNG/WebP).
/// Everything is best-effort: a missing or corrupt EXIF block is not an error.
/// Takes bytes rather than a path so it works over any storage backend.
pub fn read_metadata(bytes: &[u8]) -> AppResult<ImageMeta> {
    let mut meta = ImageMeta {
        capture_time: None,
        orientation: None,
        camera: None,
        lens: None,
        iso: None,
        width: None,
        height: None,
    };

    if let Ok(reader) = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format() {
        if let Ok((w, h)) = reader.into_dimensions() {
            meta.width = Some(w);
            meta.height = Some(h);
        }
    }

    let mut cursor = Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) else {
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
    // Raw ASCII bytes, NOT display_value(): kamadak reformats datetime tags
    // for display ("2024-06-15 …"), which silently broke capture-time parsing
    // (it expects the EXIF-native "2024:06:15 …") and made every plain image
    // fall back to mtime.
    let field_ascii = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| match &f.value {
                exif::Value::Ascii(v) => v.first().map(|b| String::from_utf8_lossy(b).into_owned()),
                _ => None,
            })
    };

    meta.capture_time = field_ascii(exif::Tag::DateTimeOriginal)
        .or_else(|| field_ascii(exif::Tag::DateTime))
        .and_then(|s| parse_exif_datetime(s.trim_end_matches('\0')));
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
