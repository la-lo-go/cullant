//! Counters for the storage work one ingest does.
//!
//! They exist to answer a question that cannot be profiled on the device where
//! it matters: is importing a project bound by *reading the files*, or by
//! everything else? On desktop a source is memory-mapped and the answer is
//! obviously "no". On Android SAF there is no mapping — every source is read
//! whole, through a descriptor obtained by a binder round-trip on the UI thread
//! — and the answer decides whether it is worth restructuring the ingest.
//!
//! Deliberately global and `Relaxed`: they are read once per phase, never
//! branched on, and must cost nothing on the hot path.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

/// Sources opened for decoding (metadata, thumbnail, preview — one each today).
static SOURCES: AtomicUsize = AtomicUsize::new(0);
/// Bytes those sources covered.
static SOURCE_BYTES: AtomicU64 = AtomicU64::new(0);
/// Wall time spent opening them, summed across workers — so it exceeds the
/// elapsed time of a parallel phase, and is compared against `workers x phase`.
static SOURCE_NANOS: AtomicU64 = AtomicU64::new(0);
/// Backend round-trips that are not source reads: SAF directory listings and
/// document-id resolutions. Zero on desktop.
static BACKEND_CALLS: AtomicUsize = AtomicUsize::new(0);

/// Record one opened decode source.
pub fn source_opened(bytes: usize, elapsed: Duration) {
    SOURCES.fetch_add(1, Ordering::Relaxed);
    SOURCE_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
    SOURCE_NANOS.fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed);
}

/// Record one non-read call into the storage backend.
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
pub fn backend_call() {
    BACKEND_CALLS.fetch_add(1, Ordering::Relaxed);
}

pub fn reset() {
    SOURCES.store(0, Ordering::Relaxed);
    SOURCE_BYTES.store(0, Ordering::Relaxed);
    SOURCE_NANOS.store(0, Ordering::Relaxed);
    BACKEND_CALLS.store(0, Ordering::Relaxed);
}

/// One line for the log: `312 sources, 8.4 GB, 41.2s in opens, 1204 backend calls`.
pub fn report() -> String {
    let bytes = SOURCE_BYTES.load(Ordering::Relaxed);
    format!(
        "{} sources, {:.2} GB, {:.1?} in opens, {} backend calls",
        SOURCES.load(Ordering::Relaxed),
        bytes as f64 / 1_073_741_824.0,
        Duration::from_nanos(SOURCE_NANOS.load(Ordering::Relaxed)),
        BACKEND_CALLS.load(Ordering::Relaxed),
    )
}
