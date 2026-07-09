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
        let params = RawDecodeParams::default();

        // (extracted image, is_full), smallest candidate first.
        let attempts: [(_, bool); 3] = [
            (self.decoder.thumbnail_image(self.source, &params), false),
            (self.decoder.preview_image(self.source, &params), false),
            (self.decoder.full_image(self.source, &params), true),
        ];

        let mut best: Option<DecodedRaw> = None;
        for (attempt, is_full) in attempts {
            match attempt {
                Ok(Some(img)) => {
                    let long = img.width().max(img.height());
                    if long >= min_long_edge {
                        return Ok(DecodedRaw {
                            image: img,
                            is_full,
                        });
                    }
                    // Keep the largest-so-far in case nothing is adequate.
                    let keep = match &best {
                        Some(b) => long > b.image.width().max(b.image.height()),
                        None => true,
                    };
                    if keep {
                        best = Some(DecodedRaw {
                            image: img,
                            is_full,
                        });
                    }
                }
                Ok(None) => continue,
                Err(e) => tracing::debug!("preview extraction step failed for {name}: {e}"),
            }
        }
        best.ok_or_else(|| AppError::Decode(format!("no embedded preview in {name}")))
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

pub struct RawMeta {
    pub capture_time: Option<i64>,
    pub orientation: Option<u16>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<u32>,
}
