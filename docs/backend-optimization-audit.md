# Backend optimization audit — Cullant (`src-tauri/`)

_Audit run 2026-07-11. Scope: the whole Rust backend, hunting **real** hot-path
wins — not style. Findings are ranked by expected impact-to-effort, with exact
`file:line` anchors. Nothing here is a rewrite; the architecture is already
well-optimized (see "What's already good" at the end)._

> **Status (2026-07-11): all four implemented** on `main`, each an atomic
> commit (`perf(thumbs): serve cached thumbnails…`, `perf(thumbs): avoid
> full-image copy…`, `perf(ingest): batch thumbnail row writes…`, `perf(db):
> add a read-only connection pool…`). Cross-platform quirks validated first
> (query-string parsing differs Windows/Android vs raw `cullant://`; SAF mtime
> can be 0 — both handled by defensive fallback; the DB always lives on real
> local storage so extra WAL connections are safe on Android). Bench (600 imgs,
> all mode): ingest ~7.97s → ~7.44s (#1/#3/#4; the #2 pool targets concurrent
> reads, not sequential ingest). #2 was built as a bounded `query_only` pool
> with `Db::call_read` after the read-your-writes consistency was confirmed and
> tested.

Legend: 🔴 high impact · 🟡 medium · ⚪ low / measure-first.

---

## 🔴 1. Thumbnail cache hits pay a serialized DB round-trip they don't need

**Where:** `protocol/mod.rs:49` (`respond_thumb`) → `thumbs::produce` (`thumbs/mod.rs:307-325`).

**What happens now:** every `cullant://thumb/{id}` request is enqueued to the
ThumbPool, and `produce()` **always** calls `file_row(db, file_id)` first
(`thumbs/mod.rs:314`) — a `query_row` that goes through the **single DB writer
thread** — purely to learn `mtime` so it can build the cache path, *before* it
ever tries `std::fs::read(&cache_abs)` (line 323).

On a fast grid scroll the webview fires hundreds of thumb requests. Every one of
them — even though the JPEG is already sitting in the disk cache — queues behind
the single DB thread (which is also busy with ingest writes, culling writes,
etc.) just to resolve a value **the URL already carries**: `api.ts:203` sends
`thumb/${id}?v=${item.mtime}`, and the handler parses only `.path()`
(`protocol/mod.rs:19`), never `.query()`.

**Fix:** read `mtime` from the `?v=` query param in the protocol handler, build
the cache path, and attempt the file read **first**. Only fall back to a
`file_row` DB lookup on a genuine cache miss (where you need `rel_path`/
`orientation`/`status` anyway to decode). Signature could become
`produce(..., known_mtime: Option<i64>)`.

**Impact:** removes the single global serialization point from the hottest
read path in the app. Cache-hit thumb serving becomes "one `fs::read`, zero DB",
which is what perceived scroll smoothness lives or dies on.

**Caveat:** `file_row` also enforces `status = 0`. A file that went missing but
whose cache lingers would still serve its thumb. That's harmless (the grid only
requests thumbs for present rows) and can be reconciled on the miss path.

---

## 🔴 2. The single DB writer thread also serializes every read

**Where:** `db/mod.rs:16-63`. One `mpsc` channel → one `Connection` on one
`db-writer` thread. **All** access, reads included, funnels through it:
`file_row` per thumb, the video `rel_path` lookup (`protocol/mod.rs:112`),
`query_items`, counts, settings…

SQLite in WAL mode (already enabled, `db/mod.rs:30`) supports **one writer +
many concurrent readers**. Today every trivial `SELECT` queues behind whatever
the writer is doing and behind every other read.

**Fix:** keep the single writer for mutations, but add a small pool of
**read-only** connections (`OpenFlags::SQLITE_OPEN_READ_ONLY`, like the
already-existing pattern in `respond_recent_thumb`, `protocol/mod.rs:238`) for
`query_row`/`query_map`. Route read closures to the pool, writes to the writer.

**Impact:** architectural ceiling-raiser. Combined with #1, a cache-hit thumb
touches **no** SQLite connection at all, and catalog/metadata reads stop
blocking on ingest writes. Bigger change than #1 — do #1 first, measure, then
decide if #2 is still needed.

---

## 🟡 3. Ingest writes thumbnail rows one commit at a time

**Where:** `scan/ingest.rs:221-230` (tier-1 loop) → `thumbs::render_and_store`
(`thumbs/mod.rs:277-297`).

Metadata is nicely **batched**: one transaction per 32-file chunk
(`write_metadata_batch`, `ingest.rs:452-494`). But the **thumbnail rows** are
not — each parallel `render_and_store` fires its own `db.call` with a single
`INSERT … ON CONFLICT` (plus a conditional `UPDATE files` for dims). So per
chunk you get ~32 individual auto-commit transactions **and** 32 channel
round-trips for thumb rows, on top of the 1 batched metadata tx.

At 50k images that's ~50k WAL commits + IPC hops for thumb rows where ~1.5k
would do.

**Fix:** have the parallel burst return the row-write payloads (it already
returns `Extracted` for metadata — extend that, or return a parallel
`Vec<ThumbRow>`), then flush them in **one transaction per chunk** on the writer
thread, exactly like `write_metadata_batch`. The JPEG encode + `fs::write` stay
parallel; only the row inserts get batched.

**Impact:** cuts commit + mpsc overhead on the biggest bulk operation (initial
import). `synchronous = NORMAL` softens per-commit fsync, so this is IPC/commit
overhead rather than fsync storms — still very worth it at scale.

---

## 🟡 4. `encode_jpeg` copies the whole image even when it's already RGB8

**Where:** `thumbs/mod.rs:458-465`.

```rust
let rgb = img.to_rgb8();   // always allocates + copies
```

After `resize_long_edge` the buffer is already `ImageRgb8` in the overwhelmingly
common path (`resize_long_edge` builds `dst` with `src.color()`, and the sources
are RGB8/RGBA8/Luma8 — `thumbs/mod.rs:437-444`). `to_rgb8()` still allocates a
fresh buffer and copies. For a 2560px **preview** that's a ~20 MB memcpy on
every single preview encode; for 384px thumbs it's small but multiplied by the
whole library.

**Fix:** `if let Some(rgb) = img.as_rgb8()` use it by reference; only
`to_rgb8()` on the exotic-format fallback.

**Related, smaller:** `resize_long_edge` returns `img.clone()` when no resize is
needed (`thumbs/mod.rs:432-434`) — a full-image clone that then gets copied
*again* by `encode_jpeg`. For already-small images this is two avoidable copies.
Low volume (only images already under the target edge), but free to fix
alongside #4.

---

## ⚪ 5. `query_items` runs three correlated subqueries per row

**Where:** `commands/catalog.rs:121-138`. Per returned row it evaluates:
`group_size` (`COUNT(*)` subquery), `tag_ids` (`GROUP_CONCAT` subquery), and
`thumb_failed` (`EXISTS` subquery). That's 3 correlated lookups × N rows.

It runs **once per catalog load** (the whole index crosses IPC once, then the
frontend virtualizes), so it's amortized and probably fine at typical shoot
sizes. But at 50k+ files it can become a visible open-time stall.

**Fix (only if measured slow):** rewrite as `LEFT JOIN … GROUP BY f.id` with
aggregates, so SQLite does one grouped pass instead of 3N subquery probes. Keep
the current version until the bench (`examples/bench_ingest.rs`) or a real large
project shows it matters — don't complicate the SQL speculatively.

---

## ⚪ 6. Scan pairing pass — correlated COUNTs (acceptable, noted)

**Where:** `scan/mod.rs:212-225`. The RAW+JPEG pairing SELECT has two
`(SELECT COUNT(*) … WHERE m.group_id = …) = 1` correlated subqueries. They're
index-backed by `idx_files_group` (`migrations.rs:43`) and the whole thing runs
**once per scan** inside the reconcile transaction, so it's fine. Flagged only
so it isn't "discovered" again later — leave it unless a profiler says otherwise.

---

## ⚪ 7. Video range buffer zero-init (negligible — do not bother)

`protocol/mod.rs:177` allocates `vec![0u8; chunk_len]` (≤8 MB) before
`read_exact` overwrites it. The zeroing is effectively free (`calloc`), and this
is per-range-request, not a tight loop. Listed so it's explicitly dismissed.

---

## What's already good (verified, don't touch)

The backend is not low-hanging fruit — these are already done right:

- **mmap-based source open** (`decode/mod.rs:24-32`): RAW/JPEG parsing pages in
  only the bytes it touches instead of reading whole sensor payloads.
- **IDCT-scaled JPEG decode** (`decode/jpeg.rs`): decodes at 1/2–1/8 when it
  beats a full decode, with a correctness-preserving fallback.
- **`decode_adequate` probes small→large embedded previews** (`decode/raw.rs:65`):
  stops at the first embedded image big enough, avoiding full demosaic.
- **Fused single-read ingest** (`scan/ingest.rs`): one read/parse/decode yields
  metadata + thumb + preview; explicitly replaced the old double-read pass.
- **`render_and_store` borrows the decode** so thumb + preview share one decode
  (`thumbs/mod.rs:239`).
- **SIMD resize** via `fast_image_resize` (`thumbs/mod.rs:427`).
- **Temp-file + rename cache writes** (`thumbs/mod.rs:268-273`) — crash-safe and
  race-safe with the background preview tier.
- **LIFO thumb queue** (`thumbs/mod.rs:32-37`) — serves what the user is looking
  at *now* first.
- **WAL + `synchronous = NORMAL`** (`db/mod.rs:30-31`).
- **Decode-failure tombstones** (`thumbs/mod.rs:364`) — undecodable files aren't
  reground every scroll.

---

## Suggested order of attack

1. **#1** (query-param mtime → skip DB on cache hit). Small, self-contained,
   biggest perceived-latency win. Measure scroll before/after.
2. **#3** (batch thumbnail row writes). Localized to ingest; speeds up import.
3. **#4** (`as_rgb8` in encode). Trivial, frees a ~20 MB copy per preview.
4. Re-measure. Only then consider **#2** (read connection pool) — the heaviest
   change, and #1 may already have removed most of the pressure that motivates
   it.

All four are behavior-preserving. Validate with `cargo test` +
`cargo clippy --all-targets -- -D warnings`, and time a real import/scroll via
`examples/bench_ingest.rs` before and after each.
