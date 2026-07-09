use std::io::Cursor;

use image::DynamicImage;

use crate::error::{AppError, AppResult};

/// Decode a JPEG so its long edge is >= `min_long_edge`, using jpeg-decoder's
/// IDCT scaling (factors 1, 1/2, 1/4, 1/8) when that lets us decode fewer
/// pixels. IDCT downscaling truncates high-frequency DCT coefficients — a
/// proper band-limited resample — so a scaled decode followed by a convolution
/// resize to the exact target is visually equivalent to a full decode + resize.
///
/// Falls back to a full decode via the image crate (zune-jpeg) whenever the
/// scaled path can't help or can't be trusted: images too small for even a 1/2
/// factor (zune is faster at scale 1), exotic pixel formats (CMYK, 16-bit),
/// any decode error, or a result that somehow undershoots the target.
pub fn decode_scaled(bytes: &[u8], min_long_edge: u32, name: &str) -> AppResult<DynamicImage> {
    match try_scaled(bytes, min_long_edge) {
        Ok(Some(img)) => return Ok(img),
        Ok(None) => {} // scaled path not applicable; fall through
        Err(e) => tracing::debug!("scaled JPEG decode failed for {name}, falling back: {e}"),
    }
    image::load_from_memory(bytes).map_err(|e| AppError::Decode(format!("{name}: {e}")))
}

/// The scaled-decode attempt. `Ok(None)` means "not applicable, use the full
/// decode" (not an error).
fn try_scaled(
    bytes: &[u8],
    min_long_edge: u32,
) -> Result<Option<DynamicImage>, jpeg_decoder::Error> {
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info()?;
    let info = match decoder.info() {
        Some(i) => i,
        None => return Ok(None),
    };
    let (full_w, full_h) = (info.width as u32, info.height as u32);

    // Only worth it when at least the 1/2 factor still meets the target:
    // jpeg-decoder is slower per pixel than zune, so it only wins when it
    // decodes <= 1/4 of the pixels.
    if full_w.max(full_h) < min_long_edge.saturating_mul(2) {
        return Ok(None);
    }

    // Aspect-correct request; scale() picks the smallest supported factor that
    // yields dimensions >= the request, so the produced long edge stays >= the
    // target regardless of the exact contract ("at least one axis" vs "both").
    let (req_w, req_h) = request_dims(full_w, full_h, min_long_edge);
    let (out_w, out_h) = decoder.scale(req_w as u16, req_h as u16)?;

    // One-directional guard: never hand back fewer pixels than asked for.
    if (out_w as u32).max(out_h as u32) < min_long_edge {
        return Ok(None);
    }

    let pixels = decoder.decode()?;
    let (w, h) = (out_w as u32, out_h as u32);
    let img = match decoder.info().map(|i| i.pixel_format) {
        Some(jpeg_decoder::PixelFormat::RGB24) => {
            image::RgbImage::from_raw(w, h, pixels).map(DynamicImage::ImageRgb8)
        }
        Some(jpeg_decoder::PixelFormat::L8) => {
            image::GrayImage::from_raw(w, h, pixels).map(DynamicImage::ImageLuma8)
        }
        // CMYK32 / L16: let the image crate handle the color conversion.
        _ => return Ok(None),
    };
    Ok(img)
}

/// Aspect-preserving dimensions whose long edge equals `long_edge` (assuming
/// the source is larger; callers check that).
fn request_dims(w: u32, h: u32, long_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    let scale = long_edge as f64 / long as f64;
    (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    /// A synthetic photo-like JPEG: gradients + deterministic noise so the
    /// equivalence comparison exercises real frequency content.
    fn test_jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let n = (x.wrapping_mul(31).wrapping_add(y.wrapping_mul(17)) % 23) as u8;
            image::Rgb([
                ((x * 255) / w) as u8,
                ((y * 255) / h) as u8,
                128u8.wrapping_add(n),
            ])
        });
        let mut out = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90);
        img.write_with_encoder(enc).unwrap();
        out
    }

    #[test]
    fn scaled_decode_engages_and_matches_full_decode() {
        let bytes = test_jpeg(4000, 3000);

        let scaled = decode_scaled(&bytes, 384, "test.jpg").unwrap();
        // The scale path must have engaged: fewer pixels than full size,
        // but still at least the requested long edge.
        assert!(scaled.width() < 4000, "expected a reduced decode");
        assert!(scaled.width().max(scaled.height()) >= 384);

        let full = image::load_from_memory(&bytes).unwrap();

        // Resize both to the same 384px target and compare.
        let a = scaled.resize_exact(384, 288, image::imageops::FilterType::Triangle);
        let b = full.resize_exact(384, 288, image::imageops::FilterType::Triangle);
        assert_eq!(a.dimensions(), b.dimensions());

        let (pa, pb) = (a.to_rgb8(), b.to_rgb8());
        let mut sum = 0u64;
        for (x, y) in pa.pixels().zip(pb.pixels()) {
            for c in 0..3 {
                sum += (x.0[c] as i16 - y.0[c] as i16).unsigned_abs() as u64;
            }
        }
        let mean = sum as f64 / (384.0 * 288.0 * 3.0);
        assert!(mean < 3.0, "mean abs diff too high: {mean}");
    }

    #[test]
    fn small_images_take_the_full_path() {
        // Long edge < 2x target: the scaled path must decline and the result
        // must be the full-size decode.
        let bytes = test_jpeg(600, 400);
        let img = decode_scaled(&bytes, 384, "small.jpg").unwrap();
        assert_eq!((img.width(), img.height()), (600, 400));
    }

    #[test]
    fn corrupt_bytes_error() {
        assert!(decode_scaled(&[0xff, 0xd8, 0x00, 0x01], 384, "bad.jpg").is_err());
    }
}
