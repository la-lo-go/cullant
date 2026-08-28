use std::io::Cursor;
use std::sync::Arc;

use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::db::Db;
use crate::decode;
use crate::error::{AppError, AppResult};
use crate::store::ProjectStore;
use crate::AppState;

/// Camera/EXIF metadata for the loupe info panel. Common fields come from the
/// indexed `files` row; the deeper photographic settings and GPS are read from
/// the actual file on demand (this command is only called when the panel is
/// open, never during scanning).
#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileMetadata {
    pub rel_path: String,
    pub kind: i64,
    pub size: i64,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub capture_time: Option<i64>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<i64>,
    pub exposure_time: Option<String>,
    pub f_number: Option<String>,
    pub focal_length: Option<String>,
    pub exposure_bias: Option<String>,
    pub flash: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
}

fn project(state: &AppState) -> AppResult<(Arc<Db>, Arc<dyn ProjectStore>)> {
    let guard = state.project.lock().unwrap();
    let p = guard.as_ref().ok_or(AppError::NoProject)?;
    Ok((p.db.clone(), p.store.clone()))
}

#[tauri::command]
pub fn get_file_metadata(file_id: i64, state: State<'_, AppState>) -> AppResult<FileMetadata> {
    let (db, store) = project(&state)?;

    let mut meta: FileMetadata = db.call(move |conn| {
        Ok(conn.query_row(
            "SELECT rel_path, kind, size, width, height, capture_time, camera, lens, iso
             FROM files WHERE id = ?1 AND status = 0",
            params![file_id],
            |r| {
                Ok(FileMetadata {
                    rel_path: r.get(0)?,
                    kind: r.get(1)?,
                    size: r.get(2)?,
                    width: r.get(3)?,
                    height: r.get(4)?,
                    capture_time: r.get(5)?,
                    camera: r.get(6)?,
                    lens: r.get(7)?,
                    iso: r.get(8)?,
                    ..Default::default()
                })
            },
        )?)
    })?;

    // Best-effort deep read; a file without EXIF still returns the DB fields.
    if let Ok(source) = decode::open_source(store.as_ref(), &meta.rel_path) {
        match meta.kind {
            0 => read_raw_exif(&source, &mut meta),
            1 => read_image_exif(source.buf(), &mut meta),
            _ => {}
        }
    }

    Ok(meta)
}

fn read_raw_exif(source: &rawler::rawsource::RawSource, meta: &mut FileMetadata) {
    use rawler::decoders::RawDecodeParams;

    let Ok(decoder) = rawler::get_decoder(source) else {
        return;
    };
    let Ok(md) = decoder.raw_metadata(source, &RawDecodeParams::default()) else {
        return;
    };
    let e = &md.exif;

    if let Some(r) = e.exposure_time {
        meta.exposure_time = Some(format_shutter(r.n as f64 / r.d.max(1) as f64));
    }
    if let Some(r) = e.fnumber {
        meta.f_number = Some(format!("f/{:.1}", r.as_f32()));
    }
    if let Some(r) = e.focal_length {
        meta.focal_length = Some(format!("{:.0} mm", r.as_f32()));
    }
    if let Some(r) = e.exposure_bias {
        let v = r.n as f64 / r.d as f64;
        if v.abs() > f64::EPSILON {
            meta.exposure_bias = Some(format!("{v:+.1} EV"));
        }
    }
    if let Some(f) = e.flash {
        meta.flash = Some(if f & 1 == 1 { "Fired" } else { "Off" }.into());
    }
    if let Some(gps) = &e.gps {
        meta.gps_lat = gps_component(gps.gps_latitude, gps.gps_latitude_ref.as_deref());
        meta.gps_lon = gps_component(gps.gps_longitude, gps.gps_longitude_ref.as_deref());
    }
}

fn gps_component(
    dms: Option<[rawler::formats::tiff::Rational; 3]>,
    reference: Option<&str>,
) -> Option<f64> {
    let dms = dms?;
    let to_f = |r: rawler::formats::tiff::Rational| r.n as f64 / r.d.max(1) as f64;
    let deg = to_f(dms[0]) + to_f(dms[1]) / 60.0 + to_f(dms[2]) / 3600.0;
    let neg = matches!(reference, Some("S") | Some("W") | Some("s") | Some("w"));
    Some(if neg { -deg } else { deg })
}

fn read_image_exif(bytes: &[u8], meta: &mut FileMetadata) {
    let mut cursor = Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) else {
        return;
    };

    let disp = |tag| {
        exif.get_field(tag, exif::In::PRIMARY)
            .map(|f| f.display_value().with_unit(&exif).to_string())
    };

    if let Some(f) = exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY) {
        if let exif::Value::Rational(ref v) = f.value {
            if let Some(r) = v.first() {
                meta.exposure_time = Some(format_shutter(r.to_f64()));
            }
        }
    }
    meta.f_number = disp(exif::Tag::FNumber).map(|s| {
        if s.starts_with("f/") {
            s
        } else {
            format!("f/{s}")
        }
    });
    meta.focal_length = disp(exif::Tag::FocalLength);
    meta.exposure_bias = disp(exif::Tag::ExposureBiasValue);
    meta.flash = exif
        .get_field(exif::Tag::Flash, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string());

    meta.gps_lat = gps_image(&exif, exif::Tag::GPSLatitude, exif::Tag::GPSLatitudeRef);
    meta.gps_lon = gps_image(&exif, exif::Tag::GPSLongitude, exif::Tag::GPSLongitudeRef);
}

fn gps_image(exif: &exif::Exif, coord: exif::Tag, reference: exif::Tag) -> Option<f64> {
    let field = exif.get_field(coord, exif::In::PRIMARY)?;
    let exif::Value::Rational(ref dms) = field.value else {
        return None;
    };
    if dms.len() < 3 {
        return None;
    }
    let deg = dms[0].to_f64() + dms[1].to_f64() / 60.0 + dms[2].to_f64() / 3600.0;
    let ref_str = exif
        .get_field(reference, exif::In::PRIMARY)
        .map(|f| f.display_value().to_string())
        .unwrap_or_default();
    let neg = ref_str.starts_with('S') || ref_str.starts_with('W');
    Some(if neg { -deg } else { deg })
}

/// Format a shutter speed in seconds the way cameras display it.
fn format_shutter(secs: f64) -> String {
    if secs <= 0.0 {
        return "—".into();
    }
    if secs >= 1.0 {
        format!("{secs:.1}s")
    } else {
        format!("1/{}", (1.0 / secs).round() as i64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shutter_formatting() {
        assert_eq!(format_shutter(0.004), "1/250");
        assert_eq!(format_shutter(2.0), "2.0s");
        assert_eq!(format_shutter(0.0), "—");
    }
}
