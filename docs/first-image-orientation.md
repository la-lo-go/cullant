# Orientation before the first image

## Cause

A new file has a NULL orientation until metadata is read.
The grid becomes available before that pass completes.
The previous renderer treated NULL as orientation 1, so early requests could generate sideways portraits.
The previous URL also used orientation 1. It could retain those pixels as an immutable image.

## Change

Unknown orientation uses `o=0` in image requests. It is a request marker, not an EXIF value.
The protocol skips immutable cache lookup for these requests and returns `Cache-Control: no-store`.
It resolves the current companion source instead of rejecting a provisional source signature that changed during metadata ingestion.
Project and file modification-time checks still apply.

When orientation is NULL, the worker reads it from the source that it decodes.
JPEG and RAW paths use the bytes already opened for image generation.
RAW+JPEG preview requests use the selected JPEG companion. A failed companion still falls back to the RAW.
The native HEIF path reuses its source orientation for the existing rotation correction.

The worker writes the detected orientation only if the database value is still NULL.
It then uses the current database value. Concurrent metadata or explicit rotation takes precedence.
It stores the generated artifact under that resolved orientation.
The regular metadata pass still supplies capture time and other fields.
It records orientation 1 when no orientation tag exists, so those files do not repeat the early-resolution work.

The loupe does not infer a rotation delta from an unknown displayed orientation.
The provisional pixels already have their source orientation applied.
Known orientations retain immediate feedback for manual rotation.

## Performance scope

This change does not wait for the whole metadata pass and does not change worker counts or pass order.
It adds orientation parsing only when an image is requested before its orientation is known.
Normal requests with known orientation retain the cache fast path and do not add a metadata read.
Early requests can add one conditional database write. Requests that arrive concurrently can repeat the small metadata parse.
RAW container metadata can cost more than JPEG EXIF parsing. A native HEIF path can need a separate source open.
The change does not claim zero work or a speedup for every first import.

## Repeatable checks

The backend regression runs the real scan and render functions while the metadata phase has not run.
It checks all eight EXIF transforms, JPEG companions, first thumbnail and preview pixels, and provisional requests after resolution.
The original renderer failed with dimensions 80x60 instead of 60x80 for orientation 6.

```powershell
cargo test --manifest-path src-tauri/Cargo.toml first_import_resolves_orientation_before_first_pixels -- --nocapture
node scripts/e2e-first-orientation.mjs artifacts/performance/orientation-fix --prepare
```

The fixture command requires Python and Pillow. It refuses an existing fixture directory.
Start an isolated app with CDP port 9223 and `CULLANT_OPEN_PROJECT` set to the printed fixture path.
After import finishes, run:

```powershell
node scripts/e2e-first-orientation.mjs artifacts/performance/orientation-fix
```

The E2E script sets orientation to NULL only in this eight-file disposable project.
This makes the pre-metadata request deterministic while retaining the real protocol and worker pool.
It checks 48 image responses: all eight orientations, three image kinds, and provisional and resolved URLs.
It checks dimensions, colour samples, cache policy, and portrait display in the loupe.
It saves `first-orientation.json` and `first-orientation.png`.
To check the metadata transition, serve Vite development assets and add `--transition`.
The script sets the catalogue orientation to unknown, opens a portrait, then supplies orientation 6.
It holds the replacement preview request and checks that the displayed provisional image remains upright.
On Windows, use `CHOKIDAR_USEPOLLING=1` if Vite tries to watch the running test executable.
It does not simulate every event ordering in a large import or test physical Android and Apple devices.

For a same-machine performance comparison, use `profile-photos.ps1 -Binary <executable>`.
Alternate the previous and changed release builds with the same preview size, cache reset, and navigation policy.
Do not compare timings while another build uses the CPU.

## Recorded verification

The Windows checks passed: 182 backend tests, Clippy with cognitive-complexity warnings enabled,
and all 48 protocol image checks. The held-preview transition kept the portrait upright.
Artifacts are in `artifacts/performance/orientation-fix/`.

The local D: regeneration runs took 32.9–36.3 seconds before the change and 34.9–35.9 seconds after it.
Both builds produced all 820 artifacts. A concurrent Android build limits this comparison.
These runs used known orientations. They do not measure early orientation resolution.
The timed release preceded the final neutral-orientation default and equivalent Clippy cleanup.
The final debug build passed the functional checks. No further performance work was requested.
