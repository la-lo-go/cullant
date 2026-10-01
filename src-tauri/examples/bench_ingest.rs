//! Time the scan + ingest pipeline against a folder, without the GUI:
//! `cargo run --release --example bench_ingest -- <dir>`
//!
//! Each run uses a new temporary database and cache. Source project data stays unchanged.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use cullant_lib::bench;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir: PathBuf = args.next().expect("usage: bench_ingest <dir>").into();

    let cache = tempfile::tempdir().expect("failed to create benchmark cache");
    let db = Arc::new(bench::open_db(cache.path()));
    let store = bench::local_store(&dir);

    let t0 = Instant::now();
    let found = bench::scan(&db, store.as_ref());
    let t_scan = t0.elapsed();

    let t1 = Instant::now();
    let (meta, thumbs, previews) = bench::ingest(&db, &store, cache.path());
    let t_ingest = t1.elapsed();

    println!("scan:   {found} files in {t_scan:.2?}");
    println!("ingest: meta={meta} thumbs={thumbs} previews={previews} in {t_ingest:.2?}");
    println!("total:  {:.2?}", t0.elapsed());
}
