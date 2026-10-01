# Photo preparation and hardware acceleration

This report concerns photo previews and thumbnails. Video acceleration is outside this study.
The objective is faster preparation without a material increase in navigation delay, memory use, or power consumption.

## Current pipeline

The background pass first prepares up to 60 thumbnails. This makes the first grid screens available early.
The main pass decodes each selected source once. It creates a preview and a thumbnail from that buffer.
A final pass prepares missing thumbnails. Valid cache entries skip generation.

A paired RAW can use its JPEG companion for previews and thumbnails.
A RAW without a companion uses an embedded image. The current path does not develop sensor pixels.
JPEG decoding uses a reduced IDCT resolution when that resolution meets the requested size.
The resize uses `fast_image_resize`, with SIMD and Rayon enabled. Orientation follows resizing.
The `image` crate encodes the results as JPEG. The app writes each cache file through a temporary file and rename.

These choices reduce work before GPU processing could start. This study preserves them.
The relevant entry points are `scan::ingest::run_ingest_inner`, `thumbs::produce_cached`, and `thumbs::render_to_cache`.

## Measurement method

Set `CULLANT_PHOTO_PROFILE` to a new JSONL path before starting the app.
An unset variable disables the counters. An existing or inaccessible path disables them and logs a warning.
The profiler never overwrites an earlier measurement.

Each record contains cumulative stage counts, total milliseconds, and maximum milliseconds.
Records also contain process elapsed time, a Unix timestamp, a process ID, and a phase event.
Measurements use a monotonic clock. Unix timestamps align them with external resource samples.

Stage durations can overlap. Never add all stage totals to obtain elapsed preparation time.

| Stage | Meaning |
| --- | --- |
| `scan` | Directory discovery, reconciliation, and grouping |
| `metadata` | Whole metadata phase, including video metadata where present |
| `source_open` | Source mapping or loading; later page faults are not included |
| `decode.*` | Source access and image decoding, grouped by selected source |
| `jpeg_decode` | JPEG attempt and fallback; nested within `decode.*` |
| `jpeg_decode.*_pixels` | Pixel decode within that JPEG operation |
| `resize.*` | Resize or no-op check, for each output size |
| `orientation.*` | Rotation and reflection, including no-op checks |
| `jpeg_encode.*` | Output JPEG encoding |
| `cache_write` | Cache path setup, directory creation, write, and rename |
| `dhash` | Similarity hash from the thumbnail |
| `db.write_wait` | Caller time through completion of a writer job, including execution |
| `db.write_execute` | Execution within the writer job; nested within caller time |
| `db.read` | Reader acquisition and read operation |
| `queue.*` | Delay from the first queued request to worker start |
| `worker` | Whole artifact request, including nested stages and cache hits |
| `pass.*` | Whole background pass, including queue waits |

Queue classification records background origin. A later interactive request can promote that work.
Coalesced late requests do not receive separate queue measurements.
Timers also include failed attempts. A no-op stage still increments its count.

The experiment uses a release backend and production web assets served through localhost:1420.
It does not use the Vite development server. This direct Cargo build retains the local frontend URL.
Its project root and cache are on D:.
It uses a separate application identifier and CDP port 9223. It does not change normal application preferences.
The runner disables video poster preparation, but discovery and video metadata remain part of import.
Source hash verification reads the media before measurement. Therefore, an empty image cache does not mean a cold Windows file cache.

## GPU feasibility

`wgpu` can provide shared native compute through DirectX 12, Vulkan, and Metal.
It does not accelerate existing JPEG or RAW decoders automatically.
See the [official platform table](https://github.com/gfx-rs/wgpu#supported-platforms).

A bounded experiment can replace resize and orientation while preserving the existing scheduling and JPEG cache.
Upload one decoded source, create both output sizes, then read back the pixels for CPU JPEG encoding.
Measure upload, dispatch, readback, and synchronization with the complete preparation time.

The current convolution filter is not equivalent to fixed bilinear texture sampling during large reductions.
Keep the filter footprint and compare fine detail before accepting a replacement.
See the [resize algorithm definitions](https://docs.rs/fast_image_resize/6.0.0/fast_image_resize/enum.ResizeAlg.html).

Native JPEG acceleration can cover more work, but adds platform constraints.
For example, [nvJPEG](https://docs.nvidia.com/cuda/nvjpeg/index.html) requires the NVIDIA ecosystem.
It does not provide one backend for AMD, Intel, Apple, and Android devices.

WIC and Android ImageDecoder do not prove GPU execution.
Android's software allocator describes output storage, not every operation inside the codec.
See [ImageDecoder](https://developer.android.com/reference/android/graphics/ImageDecoder#ALLOCATOR_SOFTWARE).

No GPU setting is added by this study. A later setting must distinguish requested policy from the effective backend.
Use device-local preferences and retain CPU fallback. A GPU failure must not mark a photo as corrupt.

### Platform scope and implementation cost

The following assessment is an engineering estimate, not a measured delivery schedule.
The native backend choices follow the [wgpu platform table](https://github.com/gfx-rs/wgpu#supported-platforms).

| Platform | Possible compute backend | Work and limits |
| --- | --- | --- |
| Windows | D3D12 or Vulkan | A resize prototype is feasible. Production support must cover integrated and discrete adapters, drivers, and device loss. WIC in this repository handles HEIF; it does not replace the JPEG pipeline. |
| Linux | Vulkan | The same shader can be reused. Adapter discovery, driver coverage, packaging, and CPU fallback need real device tests. |
| macOS | Metal | The shared compute path is feasible. The current HEIF fallback uses ffmpeg; a native Apple codec path would be a separate change. Test both output quality and transfer cost. |
| Android | Vulkan | Device support must be detected. Keep SAF access and bound memory use. Test foreground responsiveness, thermal throttling, and battery use on phones. |
| Future iOS and iPadOS | Metal | Compute is feasible in principle. The repository has no checked-in iOS target. App integration, file access, native codecs, and device tests precede a production claim. |

A single-platform resize prototype has moderate complexity. A reliable desktop and mobile backend has high complexity.
Shared shaders reduce duplication, but do not remove codec differences or platform testing.
A GPU decode-to-encode path has substantially more scope than replacing resize.
No defensible calendar estimate follows from CPU timings alone.

### Configuration and code locations

The existing user control is **Settings > Quality & performance > Preview quality**.
Its choices change the longest preview edge: 1600, 2560, or 3840 pixels.
Changing this value requires confirmation and invalidates generated previews.
This is the measured control available now. It does not select a GPU.

| Concern | Repository location |
| --- | --- |
| Preview control and confirmation | `src/lib/settingsSchema.ts`, `src/lib/components/SettingsDialog.svelte` |
| Device-local preference | `src/lib/stores/settings.svelte.ts` |
| Pass order and phase events | `src-tauri/src/scan/ingest.rs` |
| Queue, shared decode, resize, orientation, encoding, cache | `src-tauri/src/thumbs/mod.rs` |
| Scaled JPEG decode | `src-tauri/src/decode/jpeg.rs` |
| Embedded RAW preview | `src-tauri/src/decode/raw.rs` |
| Native HEIF selection | `src-tauri/src/decode/heif.rs`, `src-tauri/plugins/tauri-plugin-saf/android/src/main/java/SafPlugin.kt` |
| Optional timing output | `src-tauri/src/photo_profile.rs` |
| Repeatable experiment | `scripts/profile-photos.ps1`, `scripts/profile-photos.mjs`, [testing procedure](testing.md#measure-photo-preparation-on-a-real-disk) |

If a later prototype proves useful, put its policy beside Preview quality in Quality & performance settings.
Show the effective backend and fallback reason there. Do not imply that WebView GPU use accelerates photo generation.
Keep the implementation behind the existing worker and render functions. Reuse the combined preview and thumbnail decode.
Bound GPU jobs separately from CPU workers so that 15 workers do not allocate 15 large GPU workloads at once.
Retain cancellation, orientation, cache versioning, and CPU retry behavior.

## Experiment results

The local artifacts are stored under `artifacts/performance/2026-10-01-d-drive/`.
That directory is ignored by Git because it contains source paths and photo screenshots.
The source revision is `2dd4e6060fa3975397d56e4f354d275649def62a`, with the timing changes in this worktree.
The directory contains the binary hash, source inventory, configuration, phase traces, resource samples, results, and screenshots.

### Machine and corpus

- AMD Ryzen 7 6800HS: 8 cores and 16 logical processors.
- About 39.2 GiB of physical memory.
- NVIDIA RTX 3060 Laptop and integrated AMD Radeon graphics.
- WD_BLACK SN7100 2 TB, connected through USB, with exFAT.
- Windows Balanced power plan. Other desktop applications remained open.
- 337 RAF files and 243 JPEG files: about 20.9 GiB of photo sources.
- 170 RAW+JPEG pairs. The app generated 410 previews and 410 thumbnails.
- 50 videos remained in the catalogue. Their posters were not pregenerated.

The 15 thumbnail workers were unchanged. Rayon settings were unchanged.
The comparison does not test a GPU implementation, Linux, macOS, Android, or iOS hardware.

### Elapsed time and resources

| Run | Backend time | Metadata | Main preview pass | CPU, percent of 16 logical processors | Peak app working set |
| --- | ---: | ---: | ---: | ---: | ---: |
| First import, 2560, with navigation | 55.02 s | 18.32 s | 34.31 s | 30.9% | 806 MiB |
| Regenerate 2560, A | 28.38 s | 0.10 s | 26.19 s | 57.7% | 736 MiB |
| Regenerate 2560, B | 31.25 s | 0.12 s | 28.91 s | 63.7% | 760 MiB |
| Regenerate 2560, final control | 32.33 s | 0.10 s | 30.17 s | 61.7% | 792 MiB |
| Regenerate 1600 | 18.31 s | 0.10 s | 16.60 s | 63.9% | 406 MiB |
| Regenerate 3840 | 47.34 s | 0.12 s | 45.60 s | 55.8% | 1653 MiB |

Backend time starts at the first profiled operation and ends at `ingest_complete`.
The first run excludes about 164 seconds spent waiting for the native folder picker.
The raw UI result retains that wait. Use the backend timestamps for the first import comparison.
The first run also includes interactive decoding during metadata ingestion. Its phases overlap in worker activity.

Regeneration removes generated photo files and their thumbnail rows. It retains metadata and project state.
It is not another first import. The 2560 controls range from 28.38 to 32.33 seconds.
The run with profiling disabled took 32.02 seconds from the UI request to completion observation.
Enabled controls took 28.80, 31.63, and 32.52 seconds by the same UI measure.
This sample does not establish a precise profiler overhead. Its effect is smaller than the observed run variation.

The first run reached about 40.1 MiB/s of physical reads and 32.7 MiB/s of writes on D:.
The 2560 regeneration controls recorded no physical reads in the sampled generation intervals.
Their photo reads were served from the Windows cache. Writes still reached about 35 MiB/s.
The reported drive model does not establish the negotiated USB link speed.

Resource samples are about 1.7 to 1.9 seconds apart. The collector adds its query time to a one-second delay.
Short peaks can be missed. App memory and CPU exclude WebView processes in this table; the raw samples retain them.
Related GPU samples showed low 3D activity, consistent with display composition. They do not demonstrate photo compute acceleration.
Power draw and thermal limits were not measured.

### Where workers spend time

The following shares use run B at 2560. The denominator is 457.83 accumulated worker seconds.
This is concurrent elapsed time inside workers, not CPU time and not the 31.25-second preparation duration.

| Operation | Accumulated worker time | Share |
| --- | ---: | ---: |
| Source access and decoding | 201.45 s | 44.0% |
| JPEG encoding | 122.00 s | 26.6% |
| Cache writes and rename | 82.84 s | 18.1% |
| Resizing | 33.79 s | 7.4% |
| Orientation | 9.80 s | 2.1% |
| Hash and other work | 7.95 s | 1.7% |

Across the three 2560 controls, resizing and orientation represent 9.5% to 12.1% of worker elapsed time.
They represent 6.9% in the first import and 16.5% at 3840.
Disk wait and scheduling can occur inside these timers. These shares are not a strict limit on total speedup.

For scale only, removing a serial fraction of 9.5% to 12.1% would yield about 1.11x to 1.14x speedup.
That calculation assumes zero replacement cost. A GPU needs uploads, readback, and synchronization.
It also changes contention between concurrent workers. Only a full prototype can measure its actual benefit.

The controls performed 410 preview encodes and 470 thumbnail encodes.
The extra 60 thumbnail encodes match the deliberate lead window, followed by the combined pass.
This is bounded duplicate work, not a second decode of every photo.
The 470 decodes comprise 167 RAW requests, 172 companion JPEG requests, and 131 standalone image requests.
The latter two counts include the lead window. They are requests, not distinct source counts.

### Navigation and cache reuse

The final warm reopen completed backend work in 0.36 seconds without image regeneration.
The UI decoded its first 12 grid images in about 0.66 seconds from the open request.
Do not use that run's 6.99-second UI observation interval as import time: it includes 12 navigation checks.

The checks matched the visible preview URL to the requested file ID and waited for decoded pixels and animation frames.

| Cache state | Transitions | Median round trip | Maximum round trip | Timeouts |
| --- | ---: | ---: | ---: | ---: |
| Complete cache | 12 | 36.5 ms | 112 ms | 0 |
| Generation in progress | 12 | 80.5 ms | 890 ms | 0 |

These are short sequential navigation samples. They do not establish a latency percentile for the whole library.
Round trips include CDP communication. The first exploratory navigation checks were less strict and are not used in this table.

### Orientation finding

The first import left four preview rows with orientation 1 in their cache paths.
On reopen, the same file IDs had previews with orientation 8. Their dimensions changed from 2560x1707 to 1707x2560.
The thumbnail paths did not change in that comparison.
`orientation-findings.json` retains both sets of rows: file IDs 596, 597, 598, and 601.

Interactive decoding occurred before the metadata phase completed.
A race between early previews and metadata orientation is a plausible cause. The exact cause still needs a focused regression.
This is a cache correctness finding, not evidence that the processing architecture needs replacement.
It was not fixed during measurement. Subsequent regeneration used the established orientations.

The user also reports sideways portrait images on new projects until a later preview replaces them.
Code inspection confirms a path that can produce this symptom:
`scan/mod.rs` inserts files without orientation, and the column permits NULL.
`catalog.svelte.ts` shows the catalogue on `scan:done`, before `metadata:done`.
`thumbs::produce_cached` replaces a missing orientation with 1 before rendering.
Thus an interactive request can render an unrotated image while its EXIF orientation is still unknown.
The catalogue refreshes orientations at `metadata:done`, rather than at each metadata progress event.
`ZoomImage.svelte` can rotate the displayed image when the updated orientation arrives, before the replacement loads.
Therefore the delay does not necessarily require completion of the final-resolution preview; metadata delivery also matters.

Rechecking the saved first-import trace detects all four mismatched orientation keys.
This is a replay of recorded evidence, not a new live reproduction of every UI transition.
A fix should distinguish unknown orientation from explicit orientation 1 and resolve it before the first render.
It must preserve user rotations and XMP overrides, RAW+JPEG source selection, and native HEIF orientation handling.
Do not delay the entire grid until all metadata has finished merely to hide this path.

The follow-up [first-image orientation change](first-image-orientation.md) addresses this path.
The performance figures above describe the original measurement, before that correction.

### Decision

Keep the current scheduling and shared decode path. Do not add a GPU setting on the basis of this experiment.
Resize and orientation alone are a limited part of the measured work at 2560.
A portable GPU prototype may be useful for large previews, but it is not justified as a major speedup yet.

For a larger improvement, investigate decoding, JPEG encoding, and cache write costs first.
Do not change those paths without an equivalent-quality comparison against this corpus.
The observed storage rate also warrants checking the USB connection before changing the image algorithms.
This report does not identify the cable, enclosure, or negotiated link as the confirmed cause.

The final project remains at `D:\` with 2560-pixel previews. Normal application preferences were not changed.
Source and final cache verification results are stored with the local artifacts.

### Verification

The before and after inventories contain the same 632 source files, lengths, and modification times.
All 580 photo SHA-256 hashes match. The 1214 files in existing subproject caches retain their lengths and modification times.
The inventory excludes the new root `.cullant` directory. Video contents were not hashed.
`integrity.json` records these comparisons.

Pillow decoded all 820 final cache images. Their dimensions match SQLite, their size limits pass,
and their orientation keys match the current file rows. `cache-verification.json` records zero failures.
This checks cache consistency; it does not prove visual equivalence against a different renderer or correct colour management.

All seven instrumented generation traces passed the profile checker.
The disabled run created no profile file. The final warm reopen performed no JPEG encoding.
Both strict navigation runs loaded all 12 requested previews without a timeout.
Backend validation passed: 181 tests, formatting, and Clippy with cognitive complexity enabled and warnings denied.
The frontend check reported zero errors and zero warnings. The production frontend and release backend built successfully.
