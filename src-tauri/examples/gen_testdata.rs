//! Generate a synthetic photo folder for manual/perf testing:
//! `cargo run --release --example gen_testdata -- <out_dir> <count> [--large]`
//!
//! Every image carries a real EXIF block (DateTimeOriginal, Orientation,
//! Make/Model, ISO) so the metadata-extraction path is exercised instead of
//! the mtime fallback. Capture times are scattered so sorting by capture time
//! genuinely differs from sorting by name.
//!
//! `--large` emits 6000x4000 images (camera-sized) so decode-time downscaling
//! kicks in for both the 384px thumb and the 2560px preview; the default
//! 1600px images only exercise the thumb-scale path.

use std::path::PathBuf;

use cullant_lib::bench::{jpeg_with_exif, SyntheticExif};

fn main() {
    let mut args = std::env::args().skip(1);
    let out: PathBuf = args.next().unwrap_or_else(|| "testdata".into()).into();
    let count: u32 = args.next().and_then(|s| s.parse().ok()).unwrap_or(200);
    let large = std::env::args().any(|a| a == "--large");

    for sub in ["day1", "day2", "day2/edits"] {
        std::fs::create_dir_all(out.join(sub)).unwrap();
    }

    const ISOS: [u16; 6] = [100, 200, 400, 800, 1600, 3200];

    for i in 0..count {
        let sub = match i % 3 {
            0 => "day1",
            1 => "day2",
            _ => "day2/edits",
        };
        let (w, h) = match (large, i % 4 == 0) {
            (true, true) => (4000u32, 6000u32),
            (true, false) => (6000u32, 4000u32),
            (false, true) => (1200u32, 1600u32),
            (false, false) => (1600u32, 1200u32),
        };
        let img = image::RgbImage::from_fn(w, h, |x, y| {
            let r = ((x * 255 / w) as u8).wrapping_add((i * 37) as u8);
            let g = ((y * 255 / h) as u8).wrapping_add((i * 73) as u8);
            let b = ((i * 11) % 255) as u8;
            image::Rgb([r, g, b])
        });

        // Scatter capture times pseudo-randomly across a June 2024 shoot so
        // capture-time order differs from filename order.
        let k = (i as u64 * 7919) % (20 * 24 * 60); // minutes over ~20 days
        let (day, hour, minute) = (1 + k / 1440, (k / 60) % 24, k % 60);
        let dto = format!("2024:06:{day:02} {hour:02}:{minute:02}:00");

        // A sprinkle of EXIF-rotated files keeps the orientation path honest.
        let orientation = if i % 5 == 0 { 6 } else { 1 };

        let bytes = jpeg_with_exif(
            &img,
            &SyntheticExif {
                date_time_original: &dto,
                orientation,
                make: "Cullant",
                model: "Synth-1",
                iso: ISOS[(i % 6) as usize],
            },
        );
        let path = out.join(sub).join(format!("IMG_{i:04}.jpg"));
        std::fs::write(&path, bytes).unwrap();
        if i % 50 == 0 {
            println!("{i}/{count}");
        }
    }
    println!("done: {count} images in {}", out.display());
}
