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
    /// Focal length in millimetres.
    pub focal_length: Option<f32>,
    /// Aperture as the bare f-number (e.g. 2.8 for f/2.8).
    pub f_number: Option<f32>,
    /// Shutter speed (exposure time) in seconds.
    pub exposure_time: Option<f32>,
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
        focal_length: None,
        f_number: None,
        exposure_time: None,
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
    let field_rational = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .and_then(|f| match &f.value {
                exif::Value::Rational(v) => v.first().map(|r| r.to_f64() as f32),
                exif::Value::SRational(v) => v.first().map(|r| r.to_f64() as f32),
                _ => None,
            })
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

    // The header probe above beats EXIF when it works: it reports what the
    // pixels actually are, where EXIF reports what the camera wrote. It does
    // not work on a HEIF, which no decoder here can open, so fall back to the
    // tags. Both are pre-rotation, so `files.width`/`height` keep one meaning.
    if meta.width.is_none() {
        if let (Some(w), Some(h)) = (
            field_uint(exif::Tag::PixelXDimension),
            field_uint(exif::Tag::PixelYDimension),
        ) {
            meta.width = Some(w);
            meta.height = Some(h);
        }
    }

    meta.capture_time = [
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTimeDigitized,
        exif::Tag::DateTime,
    ]
    .into_iter()
    .find_map(|tag| field_ascii(tag).and_then(|s| parse_exif_datetime(s.trim_end_matches('\0'))));
    meta.orientation = field_uint(exif::Tag::Orientation).map(|v| v as u16);
    meta.iso = field_uint(exif::Tag::PhotographicSensitivity);
    meta.focal_length = field_rational(exif::Tag::FocalLength);
    meta.f_number = field_rational(exif::Tag::FNumber)
        .or_else(|| field_rational(exif::Tag::ApertureValue).map(|value| 2.0f32.powf(value / 2.0)))
        .filter(|value| value.is_finite() && *value > 0.0);
    meta.exposure_time = field_rational(exif::Tag::ExposureTime)
        .or_else(|| field_rational(exif::Tag::ShutterSpeedValue).map(|value| 2.0f32.powf(-value)))
        .filter(|value| value.is_finite() && *value > 0.0);

    let clean = |s: String| {
        let s = s.trim().trim_matches('"').trim().to_string();
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    };
    let make = field_str(exif::Tag::Make)
        .and_then(clean)
        .unwrap_or_default();
    let model = field_str(exif::Tag::Model)
        .and_then(clean)
        .unwrap_or_default();
    meta.camera = super::camera_name(&make, &model);
    // Raw ASCII first component, NOT display_value(): some cameras (e.g. Fuji)
    // store LensModel as a multi-string ASCII field whose extra components are
    // empty, and display_value() renders that as `"XF18-55mmF2.8-4 R LM OIS", "",
    // "", …` — the trailing quoted empties leaked into the stored lens name.
    meta.lens = field_ascii(exif::Tag::LensModel).and_then(clean);

    Ok(meta)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_regression_exif_create_date_and_signed_apex() {
        use exif::{Field, In, Tag, Value};
        let fields = [
            Field {
                tag: Tag::DateTimeOriginal,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![b"invalid".to_vec()]),
            },
            Field {
                tag: Tag::DateTimeDigitized,
                ifd_num: In::PRIMARY,
                value: Value::Ascii(vec![b"2024:06:15 14:30:05".to_vec()]),
            },
            Field {
                tag: Tag::ApertureValue,
                ifd_num: In::PRIMARY,
                value: Value::Rational(vec![exif::Rational { num: 4, denom: 1 }]),
            },
            Field {
                tag: Tag::ShutterSpeedValue,
                ifd_num: In::PRIMARY,
                value: Value::SRational(vec![exif::SRational { num: -1, denom: 1 }]),
            },
        ];
        let mut writer = exif::experimental::Writer::new();
        for field in &fields {
            writer.push_field(field);
        }
        let mut bytes = Cursor::new(Vec::new());
        writer.write(&mut bytes, false).unwrap();
        let meta = read_metadata(&bytes.into_inner()).unwrap();
        assert_eq!(meta.capture_time, Some(1718461805));
        assert_eq!(meta.f_number, Some(4.0));
        assert_eq!(meta.exposure_time, Some(2.0));
    }

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
