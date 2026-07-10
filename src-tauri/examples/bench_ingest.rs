//! Time the scan + ingest pipeline against a folder, without the GUI:
//! `cargo run --release --example bench_ingest -- <dir> [all|background|window]`
//!
//! Deletes the folder's `.cullant` sidecar first so every run is cold.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use cullant_lib::bench;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir: PathBuf = args
        .next()
        .expect("usage: bench_ingest <dir> [mode]")
        .into();
    let mode = match args.next().as_deref() {
        Some("background") => bench::PreviewMode::Background,
        Some("window") => bench::PreviewMode::Window,
        _ => bench::PreviewMode::All,
    };

    let sidecar = dir.join(".cullant");
    if sidecar.exists() {
        std::fs::remove_dir_all(&sidecar).expect("failed to clear .cullant");
    }

    let db = Arc::new(bench::open_db(&dir));
    let store = bench::local_store(&dir);

    let t0 = Instant::now();
    let found = bench::scan(&db, store.as_ref());
    let t_scan = t0.elapsed();

    let t1 = Instant::now();
    let (tier1, previews) = bench::ingest(&db, store.as_ref(), &dir, mode);
    let t_ingest = t1.elapsed();

    println!("mode: {mode:?}");
    println!("scan:   {found} files in {t_scan:.2?}");
    println!("ingest: tier1={tier1} previews(bg)={previews} in {t_ingest:.2?}");
    println!("total:  {:.2?}", t0.elapsed());
}
