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

        let camera = super::camera_name(&md.make, &md.model);
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
            focal_length: md.exif.focal_length.map(|r| r.as_f32()),
            f_number: md.exif.fnumber.map(|r| r.as_f32()),
            exposure_time: md
                .exif
                .exposure_time
                .map(|r| r.n as f32 / r.d.max(1) as f32),
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
                            if ok {
                                " -> ACCEPTED"
                            } else {
                                " (too small, keep probing)"
                            }
                        );
                        if ok {
                            return Ok(DecodedRaw {
                                image: img,
                                is_full: $is_full,
                            });
                        }
                        // Keep the largest-so-far in case nothing is adequate.
                        let keep = match &best {
                            Some((_, b)) => long > b.image.width().max(b.image.height()),
                            None => true,
                        };
                        if keep {
                            best = Some((
                                $label,
                                DecodedRaw {
                                    image: img,
                                    is_full: $is_full,
                                },
                            ));
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

        probe!(
            "thumbnail",
            self.decoder.thumbnail_image(self.source, &params),
            false
        );
        probe!(
            "preview",
            self.decoder.preview_image(self.source, &params),
            false
        );
        probe!(
            "full(full-res)",
            self.decoder.full_image(self.source, &params),
            true
        );

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
    raf_embedded_jpeg(bytes)
        .or_else(|| tiff_embedded_jpeg(bytes))
        .or_else(|| cr3_embedded_jpeg(bytes))
}

/// Fujifilm RAF: a 16-byte magic then fixed fields; at offset 0x54 a big-endian
/// u32 JPEG offset and at 0x58 its big-endian u32 length. The pointed-to bytes
/// are a standalone full-resolution JPEG (SOI = 0xFF 0xD8).
fn raf_embedded_jpeg(bytes: &[u8]) -> Option<&[u8]> {
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

// --- Little TIFF reader (byte-order aware, fully bounds-checked) ---

fn rd_u16(b: &[u8], off: usize, le: bool) -> Option<u16> {
    let s = b.get(off..off + 2)?;
    let a = [s[0], s[1]];
    Some(if le {
        u16::from_le_bytes(a)
    } else {
        u16::from_be_bytes(a)
    })
}

fn rd_u32(b: &[u8], off: usize, le: bool) -> Option<u32> {
    let s = b.get(off..off + 4)?;
    let a = [s[0], s[1], s[2], s[3]];
    Some(if le {
        u32::from_le_bytes(a)
    } else {
        u32::from_be_bytes(a)
    })
}

fn rd_u64(b: &[u8], off: usize, le: bool) -> Option<u64> {
    let s = b.get(off..off + 8)?;
    let a: [u8; 8] = s.try_into().ok()?;
    Some(if le {
        u64::from_le_bytes(a)
    } else {
        u64::from_be_bytes(a)
    })
}

/// One scalar value (BYTE/SHORT/LONG, first element) from a 12-byte IFD entry at
/// `e`. For count-1 SHORT/LONG the value is stored inline in the 4-byte field at
/// `e + 8`.
fn tiff_entry_scalar(b: &[u8], e: usize, typ: u16, le: bool) -> Option<u32> {
    match typ {
        1 => b.get(e + 8).map(|&x| u32::from(x)), // BYTE
        3 => rd_u16(b, e + 8, le).map(u32::from), // SHORT
        4 => rd_u32(b, e + 8, le),                // LONG
        _ => None,
    }
}

/// The LONG offset list of a SubIFDs (0x014A) entry: inline when count is 1,
/// otherwise the entry's value field points at `cnt` consecutive LONGs.
fn tiff_entry_long_list(b: &[u8], e: usize, cnt: u32, le: bool) -> Vec<u32> {
    let mut out = Vec::new();
    match cnt {
        0 => {}
        1 => {
            if let Some(v) = rd_u32(b, e + 8, le) {
                out.push(v);
            }
        }
        n => {
            if let Some(base) = rd_u32(b, e + 8, le) {
                for i in 0..n as usize {
                    match rd_u32(b, base as usize + i * 4, le) {
                        Some(v) => out.push(v),
                        None => break,
                    }
                }
            }
        }
    }
    out
}

/// Keep `[off, off+len)` as the new best when it is in bounds, starts with a JPEG
/// SOI, and is larger than the current best (the full-res preview is the biggest
/// embedded JPEG).
fn consider_jpeg(b: &[u8], off: usize, len: usize, best: &mut Option<(usize, usize)>) {
    if len < 2 {
        return;
    }
    let Some(end) = off.checked_add(len) else {
        return;
    };
    if end > b.len() || b[off] != 0xFF || b[off + 1] != 0xD8 {
        return;
    }
    if best.is_none_or(|(_, bl)| len > bl) {
        *best = Some((off, len));
    }
}

/// TIFF-based RAW containers (Canon CR2, Nikon NEF/NRW, Sony ARW, Adobe DNG,
/// Samsung SRW, Hasselblad 3FR, Epson ERF, Panasonic RW2, …) store one or more
/// JPEG previews inside the TIFF IFD tree. Walk IFD0, its chained IFDs and its
/// SubIFDs (bounded) and return the LARGEST displayable embedded JPEG.
///
/// Only callers with a confirmed-RAW input reach this (decode_for's RAW branch),
/// so the loose header acceptance below is safe: plain `.tif` never lands here,
/// and a false match simply finds no SOI and returns `None`.
fn tiff_embedded_jpeg(bytes: &[u8]) -> Option<&[u8]> {
    let le = match bytes.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    // Accept the standard magic (0x2A) and Panasonic RW2's (0x55); other RAW
    // TIFF variants (Olympus ORF) use exotic magics but the same IFD layout —
    // the IFD0-offset bounds check is the real guard.
    let _magic = rd_u16(bytes, 2, le)?;
    let ifd0 = rd_u32(bytes, 4, le)? as usize;
    if ifd0 < 8 || ifd0 >= bytes.len() {
        return None;
    }

    let mut best: Option<(usize, usize)> = None;
    let mut stack: Vec<usize> = vec![ifd0];
    let mut seen: Vec<usize> = Vec::new();
    let mut budget = 64u32; // cap IFDs visited (reject cyclic/malformed offsets)

    while let Some(ifd_off) = stack.pop() {
        if budget == 0 {
            break;
        }
        budget -= 1;
        if seen.contains(&ifd_off) {
            continue;
        }
        seen.push(ifd_off);

        let count = match rd_u16(bytes, ifd_off, le) {
            Some(c) => c as usize,
            None => continue,
        };
        let entries = ifd_off + 2;

        let mut compression: Option<u32> = None;
        let mut photometric: Option<u32> = None;
        let mut jpeg_if: Option<u32> = None;
        let mut jpeg_if_len: Option<u32> = None;
        let mut strip_off: Option<u32> = None;
        let mut strip_len: Option<u32> = None;
        let mut strip_count: u32 = 0;

        for i in 0..count {
            let e = entries + i * 12;
            let Some(tag) = rd_u16(bytes, e, le) else {
                break;
            };
            let Some(typ) = rd_u16(bytes, e + 2, le) else {
                break;
            };
            let Some(cnt) = rd_u32(bytes, e + 4, le) else {
                break;
            };
            match tag {
                0x0103 => compression = tiff_entry_scalar(bytes, e, typ, le),
                0x0106 => photometric = tiff_entry_scalar(bytes, e, typ, le),
                0x0111 => {
                    strip_off = tiff_entry_scalar(bytes, e, typ, le);
                    strip_count = cnt;
                }
                0x0117 => strip_len = tiff_entry_scalar(bytes, e, typ, le),
                0x0201 => jpeg_if = tiff_entry_scalar(bytes, e, typ, le),
                0x0202 => jpeg_if_len = tiff_entry_scalar(bytes, e, typ, le),
                // Panasonic RW2 "JpegFromRaw": a full-res JPEG blob at the entry's
                // offset, length = element count.
                0x002E => {
                    if let Some(off) = rd_u32(bytes, e + 8, le) {
                        consider_jpeg(bytes, off as usize, cnt as usize, &mut best);
                    }
                }
                // SubIFDs: descend into each referenced IFD.
                0x014A => {
                    for off in tiff_entry_long_list(bytes, e, cnt, le) {
                        stack.push(off as usize);
                    }
                }
                _ => {}
            }
        }

        // Chained next IFD (IFD1 = thumbnail, etc.).
        if let Some(next) = rd_u32(bytes, entries + count * 12, le) {
            let n = next as usize;
            if n >= 8 && n < bytes.len() {
                stack.push(n);
            }
        }

        // Candidate A: JPEGInterchangeFormat always points at a preview/thumb.
        if let (Some(o), Some(l)) = (jpeg_if, jpeg_if_len) {
            consider_jpeg(bytes, o as usize, l as usize, &mut best);
        }
        // Candidate B: a single-strip old-style JPEG (Compression 6) whose
        // photometric is displayable — excludes the CFA raw data (photometric
        // 32803) that is also stored as JPEG in CR2/DNG.
        if compression == Some(6) && strip_count == 1 {
            let displayable = matches!(photometric, Some(1) | Some(2) | Some(6) | None);
            if displayable {
                if let (Some(o), Some(l)) = (strip_off, strip_len) {
                    consider_jpeg(bytes, o as usize, l as usize, &mut best);
                }
            }
        }
    }

    best.map(|(o, l)| &bytes[o..o + l])
}

/// Canon CR3 is ISO-BMFF (MP4-like) boxes, not TIFF. The preview JPEG lives in a
/// `PRVW` box (a bigger `THMB` thumbnail also exists) nested inside Canon's
/// top-level `uuid` box. Walk the box tree (descending only into container boxes
/// so we never parse `mdat` media as boxes) and return the largest embedded JPEG.
///
/// Note: CR3's PRVW is a reduced preview (~1620px), smaller than the sensor — so
/// the loupe preview is slightly softer than a full-res decode, in exchange for
/// avoiding rawler's multi-second full-res path. Acceptable per the plan.
fn cr3_embedded_jpeg(bytes: &[u8]) -> Option<&[u8]> {
    // Gate on an ftyp box whose brand set mentions "crx " (Canon CR3/CRM).
    if bytes.get(4..8)? != b"ftyp" {
        return None;
    }
    let ftyp_end = rd_u32(bytes, 0, false)? as usize;
    let ftyp_end = ftyp_end.min(bytes.len()).max(8);
    if !bytes.get(8..ftyp_end)?.windows(4).any(|w| w == b"crx ") {
        return None;
    }

    let mut best: Option<(usize, usize)> = None;
    cr3_walk(bytes, 0, bytes.len(), 0, &mut best);
    best.map(|(o, l)| &bytes[o..o + l])
}

/// Recurse over BMFF boxes in `[pos, end)`, collecting the largest JPEG found in
/// any `PRVW`/`THMB` box. Descends only into `moov`/`uuid` containers.
fn cr3_walk(b: &[u8], mut pos: usize, end: usize, depth: u32, best: &mut Option<(usize, usize)>) {
    if depth > 6 {
        return;
    }
    while pos + 8 <= end {
        let Some(size32) = rd_u32(b, pos, false) else {
            return;
        };
        let typ = &b[pos + 4..pos + 8];
        // Resolve the box's content start and end (handle 64-bit and to-EOF).
        let (content, box_end) = match size32 {
            1 => match rd_u64(b, pos + 8, false) {
                Some(large) => (pos + 16, pos.checked_add(large as usize)),
                None => return,
            },
            0 => (pos + 8, Some(end)),
            n => (pos + 8, pos.checked_add(n as usize)),
        };
        let Some(box_end) = box_end else {
            return;
        };
        if box_end <= pos || box_end > end || content > box_end {
            return;
        }
        match typ {
            b"moov" => cr3_walk(b, content, box_end, depth + 1, best),
            // uuid: skip the 16-byte UUID, then recurse into nested boxes.
            b"uuid" => {
                let inner = content + 16;
                if inner <= box_end {
                    cr3_walk(b, inner, box_end, depth + 1, best);
                }
            }
            b"PRVW" | b"THMB" => {
                if let Some(j) = cr3_find_jpeg(b, content, box_end) {
                    consider_jpeg(b, j, box_end - j, best);
                }
            }
            _ => {}
        }
        pos = box_end;
    }
}

/// Find a JPEG SOI (`FF D8`) within a small box payload `[start, end)`.
fn cr3_find_jpeg(b: &[u8], start: usize, end: usize) -> Option<usize> {
    let slice = b.get(start..end)?;
    slice
        .windows(2)
        .position(|w| w == [0xFF, 0xD8])
        .map(|p| start + p)
}

pub struct RawMeta {
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

    // --- TIFF walker ---

    /// One little-endian IFD: a count, `entries` (tag, type, count, inline value)
    /// each 12 bytes, then the `next` IFD offset.
    fn ifd(entries: &[(u16, u16, u32, u32)], next: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for &(tag, typ, cnt, val) in entries {
            v.extend_from_slice(&tag.to_le_bytes());
            v.extend_from_slice(&typ.to_le_bytes());
            v.extend_from_slice(&cnt.to_le_bytes());
            v.extend_from_slice(&val.to_le_bytes()); // count-1 SHORT/LONG stored inline
        }
        v.extend_from_slice(&next.to_le_bytes());
        v
    }

    /// A little-endian TIFF RAW with three SubIFDs: a small preview JPEG, a larger
    /// preview JPEG, and a CFA (raw) IFD whose strip is the BIGGEST JPEG-looking
    /// blob but must be excluded (photometric 32803). Fixed offsets:
    ///   0   header (8)         20  IFD0 (18)        68  SubIFD B (30)
    ///   8   subifd ptrs (12)   38  SubIFD A (30)    98  CFA IFD (54)
    ///   152 jpeg_small(4) 156 jpeg_large(8) 164 jpeg_cfa(20)
    fn fake_tiff() -> Vec<u8> {
        let (a, b, cfa) = (38u32, 68u32, 98u32);
        let (small, large, cfa_j) = (152u32, 156u32, 164u32);

        let mut buf = vec![0u8; 8];
        buf[0..2].copy_from_slice(b"II");
        buf[2..4].copy_from_slice(&0x2Au16.to_le_bytes());
        buf[4..8].copy_from_slice(&20u32.to_le_bytes()); // IFD0 offset

        // subifd pointer array at 8: [A, B, CFA]
        for off in [a, b, cfa] {
            buf.extend_from_slice(&off.to_le_bytes());
        }
        assert_eq!(buf.len(), 20);
        // IFD0: SubIFDs (0x014A, LONG, count 3, value = ptr array @ 8)
        buf.extend_from_slice(&ifd(&[(0x014A, 4, 3, 8)], 0));
        assert_eq!(buf.len(), 38);
        // SubIFD A: JPEGInterchangeFormat + length → small
        buf.extend_from_slice(&ifd(&[(0x0201, 4, 1, small), (0x0202, 4, 1, 4)], 0));
        assert_eq!(buf.len(), 68);
        // SubIFD B: JPEGInterchangeFormat + length → large
        buf.extend_from_slice(&ifd(&[(0x0201, 4, 1, large), (0x0202, 4, 1, 8)], 0));
        assert_eq!(buf.len(), 98);
        // CFA IFD: Compression 6, Photometric 32803 (CFA), single strip → cfa_j
        buf.extend_from_slice(&ifd(
            &[
                (0x0103, 3, 1, 6),
                (0x0106, 3, 1, 32803),
                (0x0111, 4, 1, cfa_j),
                (0x0117, 4, 1, 20),
            ],
            0,
        ));
        assert_eq!(buf.len(), 152);
        buf.extend_from_slice(&[0xFF, 0xD8, 0xFF, 0xD9]); // small (4)
        buf.extend_from_slice(&[0xFF, 0xD8, 0, 0, 0, 0, 0xFF, 0xD9]); // large (8)
        let mut cfa_blob = vec![0xFFu8, 0xD8];
        cfa_blob.extend_from_slice(&[0u8; 16]);
        cfa_blob.extend_from_slice(&[0xFF, 0xD9]);
        buf.extend_from_slice(&cfa_blob); // cfa (20, biggest but excluded)
        buf
    }

    #[test]
    fn tiff_picks_largest_displayable_preview_excluding_cfa() {
        let tiff = fake_tiff();
        let got = embedded_jpeg(&tiff).expect("a preview");
        // The 8-byte SubIFD-B preview, not the 20-byte CFA strip nor the 4-byte one.
        assert_eq!(got, &[0xFF, 0xD8, 0, 0, 0, 0, 0xFF, 0xD9]);
    }

    #[test]
    fn tiff_rejects_bad_header_and_out_of_bounds() {
        // Valid byte order but IFD0 offset past the end.
        let mut t = vec![0u8; 8];
        t[0..2].copy_from_slice(b"II");
        t[2..4].copy_from_slice(&0x2Au16.to_le_bytes());
        t[4..8].copy_from_slice(&9999u32.to_le_bytes());
        assert_eq!(embedded_jpeg(&t), None);

        // Not a TIFF/RAF/CR3 container at all.
        assert_eq!(embedded_jpeg(&[0u8; 64]), None);
    }

    // --- CR3 (ISO-BMFF) ---

    /// Minimal CR3: an `ftyp` box with the `crx ` brand, then a `uuid` box holding
    /// a nested `PRVW` box whose payload contains a JPEG.
    fn fake_cr3(jpeg: &[u8]) -> Vec<u8> {
        // box = size(u32 BE) + fourcc + payload
        fn boxed(fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let size = 8 + payload.len();
            let mut v = (size as u32).to_be_bytes().to_vec();
            v.extend_from_slice(fourcc);
            v.extend_from_slice(payload);
            v
        }
        // PRVW: a small header then the JPEG.
        let mut prvw_payload = vec![0u8; 6];
        prvw_payload.extend_from_slice(jpeg);
        let prvw = boxed(b"PRVW", &prvw_payload);
        // uuid box: 16-byte UUID + the PRVW box.
        let mut uuid_payload = vec![0u8; 16];
        uuid_payload.extend_from_slice(&prvw);
        let uuid = boxed(b"uuid", &uuid_payload);
        // ftyp: major brand "crx ", minor, one compatible brand.
        let ftyp = boxed(b"ftyp", b"crx \0\0\0\0crx ");

        let mut buf = ftyp;
        buf.extend_from_slice(&uuid);
        buf
    }

    #[test]
    fn extracts_cr3_prvw_jpeg() {
        let jpeg = [0xFFu8, 0xD8, 0xAA, 0xBB, 0xFF, 0xD9];
        let cr3 = fake_cr3(&jpeg);
        assert_eq!(embedded_jpeg(&cr3), Some(&jpeg[..]));
    }

    #[test]
    fn cr3_without_crx_brand_is_ignored() {
        // An MP4 that is not a CR3 must not be treated as one.
        let mut buf = Vec::new();
        let ftyp = {
            let payload = b"isom\0\0\0\0isommp42";
            let size = (8 + payload.len()) as u32;
            let mut v = size.to_be_bytes().to_vec();
            v.extend_from_slice(b"ftyp");
            v.extend_from_slice(payload);
            v
        };
        buf.extend_from_slice(&ftyp);
        assert_eq!(embedded_jpeg(&buf), None);
    }
}
