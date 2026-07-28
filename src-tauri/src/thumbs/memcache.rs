//! A small in-memory cache of already-encoded grid thumbnails.
//!
//! It exists because of one Android-only fact: wry's `RustWebViewClient`
//! overwrites the `Cache-Control: immutable` header the protocol handler sets
//! with `no-store`. The WebView therefore keeps nothing, and every cell that
//! scrolls out and back re-requests its thumbnail through the whole protocol
//! path. During an import those re-requests queue behind the pregeneration
//! pass, and the cell sits empty — the grid visibly loses thumbnails it had
//! already painted.
//!
//! Serving those from memory turns the round trip into a `memcpy`. Only grid
//! thumbnails are held: they are tens of kilobytes and there are hundreds on
//! screen, whereas a preview is hundreds of kilobytes and is looked at one at a
//! time.
//!
//! Keyed by cache-relative path, which already encodes file id, mtime,
//! orientation and (for previews) target size — so a changed or rotated photo
//! can never be served a stale entry. File ids are per project, though, so the
//! whole cache is dropped when a project closes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// Total encoded bytes held. ~24 MB is several screenfuls of 384px JPEGs and is
/// small next to a single in-flight preview decode.
const BUDGET_BYTES: usize = 24 * 1024 * 1024;

/// How far to evict past the budget, so eviction is occasional rather than
/// once per insert.
const EVICT_TO: usize = BUDGET_BYTES * 3 / 4;

struct Entry {
    bytes: Arc<Vec<u8>>,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<String, Entry>,
    bytes: usize,
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
static CLOCK: AtomicU64 = AtomicU64::new(0);

fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}

/// Bytes for `cache_rel`, if held. Marks the entry as just used.
pub fn get(cache_rel: &str) -> Option<Arc<Vec<u8>>> {
    let mut c = cache().lock().ok()?;
    let entry = c.entries.get_mut(cache_rel)?;
    entry.used = CLOCK.fetch_add(1, Ordering::Relaxed);
    Some(entry.bytes.clone())
}

/// Hold `bytes` for `cache_rel`, evicting least-recently-used entries if that
/// puts the cache over budget. Oversized payloads are simply not held.
pub fn put(cache_rel: &str, bytes: Arc<Vec<u8>>) {
    if bytes.len() > BUDGET_BYTES / 8 {
        return;
    }
    let Ok(mut c) = cache().lock() else { return };
    if let Some(old) = c.entries.remove(cache_rel) {
        c.bytes -= old.bytes.len();
    }
    c.bytes += bytes.len();
    let used = CLOCK.fetch_add(1, Ordering::Relaxed);
    c.entries
        .insert(cache_rel.to_string(), Entry { bytes, used });

    if c.bytes <= BUDGET_BYTES {
        return;
    }
    let mut by_age: Vec<(u64, String)> =
        c.entries.iter().map(|(k, e)| (e.used, k.clone())).collect();
    by_age.sort_unstable_by_key(|(used, _)| *used);
    for (_, key) in by_age {
        if c.bytes <= EVICT_TO {
            break;
        }
        if let Some(e) = c.entries.remove(&key) {
            c.bytes -= e.bytes.len();
        }
    }
}

/// Drop everything. Called when a project closes: cache paths embed file ids,
/// which are only unique within one project's database.
pub fn clear() {
    if let Ok(mut c) = cache().lock() {
        c.entries.clear();
        c.bytes = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cache is process-global, so tests that fill it would clear each
    /// other's entries mid-run.
    static TESTS: Mutex<()> = Mutex::new(());

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        TESTS.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn holds_and_returns_bytes() {
        let _g = guard();
        clear();
        put("a/1_t.jpg", Arc::new(vec![1, 2, 3]));
        assert_eq!(get("a/1_t.jpg").as_deref(), Some(&vec![1, 2, 3]));
        assert!(get("a/2_t.jpg").is_none());
    }

    #[test]
    fn evicts_least_recently_used_first() {
        let _g = guard();
        clear();
        // Comfortably under the per-entry ceiling, so every put is accepted;
        // ten of them fill the budget exactly.
        let chunk = BUDGET_BYTES / 10;
        assert!(
            chunk < BUDGET_BYTES / 8,
            "chunks must not be refused as oversized"
        );
        for i in 0..10 {
            put(&format!("k{i}"), Arc::new(vec![0u8; chunk]));
        }
        // Nothing is probed here on purpose: `get` is an LRU touch, not a
        // read-only peek, so checking a key would be enough to rescue it from
        // being the oldest.
        //
        // Touch k1 so it is no longer among the oldest, then push past the
        // budget so eviction has to choose.
        let _ = get("k1");
        put("k10", Arc::new(vec![0u8; chunk]));

        assert!(get("k0").is_none(), "the least recently used goes first");
        assert!(get("k1").is_some(), "a recently used entry survives");
        assert!(get("k10").is_some(), "the newest is held");
    }

    #[test]
    fn refuses_an_entry_too_large_to_be_worth_holding() {
        let _g = guard();
        clear();
        put("huge", Arc::new(vec![0u8; BUDGET_BYTES / 8 + 1]));
        assert!(get("huge").is_none());
    }

    #[test]
    fn clearing_drops_everything() {
        let _g = guard();
        clear();
        put("x", Arc::new(vec![7; 16]));
        clear();
        assert!(get("x").is_none());
    }
}
