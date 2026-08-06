//! The Windows rung of the HEIF ladder: Windows Imaging Component.
//!
//! Pure Rust bindings to DLLs that ship with the OS — no C toolchain and no
//! bundled library. The codec itself is not part of Windows though: it arrives
//! with Microsoft's HEIF/HEVC extensions, which many machines do not have. So
//! [`available`] enumerates WIC's registered decoders and looks for the HEIF
//! container format, rather than finding out by failing on a photo.

use std::path::Path;
use std::sync::OnceLock;

use image::{DynamicImage, RgbImage};
use windows::core::{Interface, HSTRING};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatHeif, GUID_WICPixelFormat24bppBGR,
    IWICBitmapDecoderInfo, IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone,
    WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICComponentEnumerateDefault,
    WICDecodeMetadataCacheOnDemand, WICDecoder,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};

use crate::error::{AppError, AppResult};

thread_local! {
    /// COM must be initialised on every thread that calls WIC, and the thumbnail
    /// pool spawns its own. `RPC_E_CHANGED_MODE` means another initialiser won
    /// with a different apartment, which WIC is happy to live in — so anything
    /// but a hard failure counts as ready.
    static COM_READY: bool = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() }
        || cfg!(windows);
}

fn factory() -> AppResult<IWICImagingFactory> {
    COM_READY.with(|_| ());
    unsafe { CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER) }
        .map_err(|e| AppError::Decode(format!("WIC factory: {e}")))
}

/// Whether WIC has a decoder registered for the HEIF container.
///
/// Asked without a file and cached: the answer is a property of the machine's
/// installed codecs, and the extensions cannot appear mid-session.
pub(super) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| has_heif_codec().unwrap_or(false))
}

fn has_heif_codec() -> AppResult<bool> {
    let factory = factory()?;
    let enumerator = unsafe {
        factory
            .CreateComponentEnumerator(WICDecoder.0 as u32, WICComponentEnumerateDefault.0 as u32)
    }
    .map_err(|e| AppError::Decode(format!("WIC enumerator: {e}")))?;

    loop {
        let mut found = [None];
        let mut fetched = 0u32;
        if unsafe { enumerator.Next(&mut found, Some(&mut fetched)) }.is_err() || fetched == 0 {
            return Ok(false);
        }
        let Some(info) = found[0].as_ref() else {
            return Ok(false);
        };
        let Ok(info) = info.cast::<IWICBitmapDecoderInfo>() else {
            continue;
        };
        if unsafe { info.GetContainerFormat() }.is_ok_and(|g| g == GUID_ContainerFormatHeif) {
            return Ok(true);
        }
    }
}

/// Decode `path`, scaled so its long edge is at least `min_long_edge` — the
/// caller resizes to the exact size. Also returns the frame's ORIGINAL
/// dimensions, which the scaled bitmap no longer carries.
///
/// WIC honours the container's `irot`, so the frame comes back already rotated
/// and the caller must not apply the EXIF orientation on top.
pub(super) fn decode(path: &Path, min_long_edge: u32) -> AppResult<(DynamicImage, (u32, u32))> {
    let factory = factory()?;
    let decoder = unsafe {
        factory.CreateDecoderFromFilename(
            &HSTRING::from(path),
            None,
            windows::Win32::Foundation::GENERIC_READ,
            WICDecodeMetadataCacheOnDemand,
        )
    }
    .map_err(|e| AppError::Decode(format!("{}: WIC decoder: {e}", path.display())))?;

    let frame = unsafe { decoder.GetFrame(0) }
        .map_err(|e| AppError::Decode(format!("{}: WIC frame: {e}", path.display())))?;

    let (mut w, mut h) = (0u32, 0u32);
    unsafe { frame.GetSize(&mut w, &mut h) }
        .map_err(|e| AppError::Decode(format!("{}: WIC size: {e}", path.display())))?;
    let original = (w, h);
    if w == 0 || h == 0 {
        return Err(AppError::Decode(format!(
            "{}: WIC empty frame",
            path.display()
        )));
    }

    // Scale during decode when the caller wants far less than the full frame:
    // WIC then skips most of the work rather than handing over 48 MP to throw
    // away. `u32::MAX` (the focus check) asks for everything.
    let long = w.max(h);
    let source: IWICBitmapSource = if min_long_edge < long && min_long_edge != u32::MAX {
        let scale = min_long_edge as f64 / long as f64;
        let (tw, th) = (
            ((w as f64 * scale).round() as u32).max(1),
            ((h as f64 * scale).round() as u32).max(1),
        );
        let scaler = unsafe { factory.CreateBitmapScaler() }
            .map_err(|e| AppError::Decode(format!("WIC scaler: {e}")))?;
        unsafe { scaler.Initialize(&frame, tw, th, WICBitmapInterpolationModeFant) }
            .map_err(|e| AppError::Decode(format!("WIC scaler init: {e}")))?;
        w = tw;
        h = th;
        scaler.into()
    } else {
        frame.into()
    };

    let converter = unsafe { factory.CreateFormatConverter() }
        .map_err(|e| AppError::Decode(format!("WIC converter: {e}")))?;
    unsafe {
        converter.Initialize(
            &source,
            &GUID_WICPixelFormat24bppBGR,
            WICBitmapDitherTypeNone,
            None,
            0.0,
            WICBitmapPaletteTypeCustom,
        )
    }
    .map_err(|e| AppError::Decode(format!("WIC convert: {e}")))?;

    let stride = w
        .checked_mul(3)
        .ok_or_else(|| AppError::Decode(format!("{}: WIC frame too wide", path.display())))?;
    let len = (stride as usize)
        .checked_mul(h as usize)
        .ok_or_else(|| AppError::Decode(format!("{}: WIC frame too large", path.display())))?;
    let mut buf = vec![0u8; len];
    unsafe { converter.CopyPixels(std::ptr::null(), stride, &mut buf) }
        .map_err(|e| AppError::Decode(format!("{}: WIC copy: {e}", path.display())))?;

    // WIC gives BGR; `image` wants RGB.
    for px in buf.chunks_exact_mut(3) {
        px.swap(0, 2);
    }
    let img = RgbImage::from_raw(w, h, buf)
        .ok_or_else(|| AppError::Decode(format!("{}: WIC buffer size", path.display())))?;
    Ok((DynamicImage::ImageRgb8(img), original))
}
