use image::DynamicImage;
use rawler::decoders::{Decoder, RawDecodeParams};
use rawler::rawsource::RawSource;

use crate::error::{AppError, AppResult};

/// A decoded embedded image plus whether it was the container's full-size one
/// (only then do its dimensions approximate the original file's dimensions,
/// making them safe to record as `files.width/height`).
pub struct DecodedRaw {
    pub image: DynamicImage,
    pub is_full: bool,
}

/// One parsed RAW container serving both metadata and pixel extraction, so an
/// ingest pass never parses the same file twice.
pub struct RawSession<'a> {
    source: &'a RawSource,
    decoder: Box<dyn Decoder>,
}

impl<'a> RawSession<'a> {
    pub fn open(source: &'a RawSource) -> AppResult<Self> {
        let decoder = rawler::get_decoder(source).map_err(|e| AppError::Decode(format!("{e}")))?;
        Ok(RawSession { source, decoder })
    }

    /// Camera metadata from the already-parsed container (no pixel decode).
    pub fn metadata(&self, _name: &str) -> AppResult<RawMeta> {
        let md = self
            .decoder
            .raw_metadata(self.source, &RawDecodeParams::default())
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

    /// Decode the smallest embedded image whose long edge is >= `min_long_edge`,
    /// probing small → large (thumbnail, preview, full) and stopping at the
    /// first adequate hit — decoding a 160px thumbnail or a ~2MP preview to
    /// check its size is far cheaper than always decoding the full-size
    /// embedded JPEG. If nothing reaches the target, the largest image found is
    /// returned (worst case equals decoding the full image up front), so
    /// `min_long_edge == u32::MAX` means "give me the largest available".
    pub fn decode_adequate(&self, min_long_edge: u32, name: &str) -> AppResult<DecodedRaw> {
        use std::time::Instant;

        let params = RawDecodeParams::default();
        let mut best: Option<(&str, DecodedRaw)> = None;

        // Probe smallest → largest, timing each tier and STOPPING at the first
        // image whose long edge meets the target. In rawler 0.7.2 `full_image`
        // does NOT demosaic — it extracts the embedded JPEG but decodes it at
        // FULL resolution (image::load_from_memory), which under the pool's many
        // concurrent workers is far slower than an IDCT-scaled decode. So it must
        // only run when no smaller embedded thumbnail/preview is adequate. (The
        // previous eager array literal evaluated all three every time, forcing a
        // full-res decode of every RAW unconditionally.)
        macro_rules! probe {
            ($label:expr, $call:expr, $is_full:expr) => {{
                let t0 = Instant::now();
                let attempt = $call;
                let ms = t0.elapsed().as_millis();
                match attempt {
                    Ok(Some(img)) => {
                        let long = img.width().max(img.height());
                        let ok = long >= min_long_edge;
                        tracing::debug!(
                            "raw decode {name}: tier={} dims={}x{} ({ms}ms){}",
                            $label,
                            img.width(),
                            img.height(),
                            if ok { " -> ACCEPTED" } else { " (too small, keep probing)" }
                        );
                        if ok {
                            return Ok(DecodedRaw { image: img, is_full: $is_full });
                        }
                        // Keep the largest-so-far in case nothing is adequate.
                        let keep = match &best {
                            Some((_, b)) => long > b.image.width().max(b.image.height()),
                            None => true,
                        };
                        if keep {
                            best = Some(($label, DecodedRaw { image: img, is_full: $is_full }));
                        }
                    }
                    Ok(None) => {
                        tracing::debug!("raw decode {name}: tier={} unavailable ({ms}ms)", $label)
                    }
                    Err(e) => {
                        tracing::debug!("raw decode {name}: tier={} FAILED ({ms}ms): {e}", $label)
                    }
                }
            }};
        }

        probe!("thumbnail", self.decoder.thumbnail_image(self.source, &params), false);
        probe!("preview", self.decoder.preview_image(self.source, &params), false);
        probe!("full(full-res)", self.decoder.full_image(self.source, &params), true);

        match best {
            Some((label, raw)) => {
                tracing::debug!(
                    "raw decode {name}: nothing met min_edge={min_long_edge}; using largest tier={label} ({}x{})",
                    raw.image.width(),
                    raw.image.height()
                );
                Ok(raw)
            }
            None => Err(AppError::Decode(format!("no embedded preview in {name}"))),
        }
    }
}

/// One-shot variant of [`RawSession::decode_adequate`].
pub fn embedded_preview_scaled(
    source: &RawSource,
    min_long_edge: u32,
    name: &str,
) -> AppResult<DecodedRaw> {
    RawSession::open(source)?.decode_adequate(min_long_edge, name)
}

/// Extract a full-resolution embedded JPEG from a RAW container when the format
/// stores one at a known offset. Fujifilm `.RAF` files carry a full-size camera
/// JPEG whose offset/length live in the fixed header — but rawler's RAF decoder
/// exposes neither `thumbnail_image` nor `preview_image`, and its `full_image`
/// only returns the same embedded JPEG after decoding it at FULL resolution
/// (7–13 s per file under the pool's concurrency, measured on real RAFs).
/// Returning the raw bytes lets the caller IDCT-scale the decode instead.
/// Returns the JPEG as a sub-slice of `bytes`, or `None` when the container
/// isn't a recognized embedded-JPEG format or the pointer doesn't land on a JPEG.
///
/// This is the Photo Mechanic "fast path" for these bodies: extract, then decode
/// exactly like a plain JPEG (scaled IDCT + convolution resize).
pub fn embedded_jpeg(bytes: &[u8]) -> Option<&[u8]> {
    // --- Fujifilm RAF ---
    // Layout: a 16-byte magic, then fixed fields; at offset 0x54 a big-endian
    // u32 JPEG offset and at 0x58 its big-endian u32 length. The pointed-to
    // bytes are a standalone JPEG (SOI = 0xFF 0xD8).
    const RAF_MAGIC: &[u8] = b"FUJIFILMCCD-RAW ";
    if bytes.len() >= 92 && bytes.starts_with(RAF_MAGIC) {
        let off = u32::from_be_bytes(bytes[84..88].try_into().ok()?) as usize;
        let len = u32::from_be_bytes(bytes[88..92].try_into().ok()?) as usize;
        let end = off.checked_add(len)?;
        if len >= 2 && end <= bytes.len() && bytes[off] == 0xFF && bytes[off + 1] == 0xD8 {
            return Some(&bytes[off..end]);
        }
    }
    None
}

pub struct RawMeta {
    pub capture_time: Option<i64>,
    pub orientation: Option<u16>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal RAF byte blob whose header points at `jpeg` placed at
    /// offset 128 (past the fixed header region).
    fn fake_raf(jpeg: &[u8]) -> Vec<u8> {
        let off: u32 = 128;
        let mut buf = vec![0u8; off as usize];
        buf[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
        buf[84..88].copy_from_slice(&off.to_be_bytes());
        buf[88..92].copy_from_slice(&(jpeg.len() as u32).to_be_bytes());
        buf.extend_from_slice(jpeg);
        buf
    }

    #[test]
    fn extracts_embedded_raf_jpeg() {
        let jpeg = [0xFFu8, 0xD8, 0x11, 0x22, 0x33, 0xFF, 0xD9];
        let raf = fake_raf(&jpeg);
        assert_eq!(embedded_jpeg(&raf), Some(&jpeg[..]));
    }

    #[test]
    fn rejects_non_raf_and_bad_pointers() {
        // Not a RAF at all.
        assert_eq!(embedded_jpeg(b"not a raw file, just bytes here..."), None);

        // RAF magic but the pointer overruns the buffer.
        let mut raf = vec![0u8; 92];
        raf[..16].copy_from_slice(b"FUJIFILMCCD-RAW ");
        raf[84..88].copy_from_slice(&1000u32.to_be_bytes());
        raf[88..92].copy_from_slice(&50u32.to_be_bytes());
        assert_eq!(embedded_jpeg(&raf), None);

        // RAF magic, in-bounds pointer, but the target isn't a JPEG (no SOI).
        let not_jpeg = [0x00u8, 0x01, 0x02, 0x03];
        let raf = fake_raf(&not_jpeg);
        assert_eq!(embedded_jpeg(&raf), None);
    }
}
