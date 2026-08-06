# Testing Cullant

122 backend tests, no frontend ones. This describes what each layer is for, what
it cannot tell you, and which checks have to be run by hand because no machine
in CI has the hardware.

## The checks before a commit

```sh
cd src-tauri
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test

cd ..
npm run check          # must end 0 errors, 0 warnings
```

Run the app for anything with runtime surface. A passing suite has never been
sufficient here: the decoder gaps in "Real camera files" below all passed CI.

## Three kinds of test data, and what each is good for

### 1. Hand-built inputs, in the test itself

Most of the 122. A `tempfile::tempdir()`, a few `touch`ed files, and an
assertion about the database.

Right for: grouping, ordering, culling state, the action queue, the commit step,
XMP round-trips, SQL predicates. Anything whose truth is a row.

Wrong for: anything about pixels or metadata. `touch(root, "IMG_1.CR3")` writes
the bytes `raw` — that is a fine stand-in for a file that must be counted,
grouped and deleted, and no stand-in at all for one that must be decoded.

### 2. Synthetic images

```sh
cd src-tauri && cargo run --release --example gen_testdata -- C:\dev\cullant-testdata 600
```

`bench::jpeg_with_exif` (`src-tauri/src/lib.rs`) builds a real JPEG with a real
EXIF block, using the same library the app reads with, so a round-trip is
guaranteed. `bench::heif_with_exif` does the same for a HEIF container — `ftyp`,
`meta`, `iloc`, `iinf`/`infe`, `idat` — carrying EXIF and no HEVC at all.

Right for: the metadata pipeline end to end, ingest ordering, thumbnail counts,
cache behaviour, sort and filter facets.

Wrong for: the decoder. A synthetic JPEG has no maker notes, no embedded preview
at a vendor's private offset, no `irot` that disagrees with EXIF, and no sibling
that belongs to the same shot.

### 3. Real camera files

```sh
npm run fixtures                          # ~90 MB of CC0 samples
cd src-tauri && cargo test the_corpus -- --nocapture
```

See [`fixtures/README.md`](../fixtures/README.md). The corpus is fetched, not
committed: covering every RAW format is a few hundred megabytes and git keeps
every byte forever. `manifest.json` is tracked and carries a SHA-256 per file,
which the fetcher verifies before *and* after writing.

`the_corpus_of_real_files_is_read_and_rendered` copies whatever is in
`fixtures/media/` into a temp project, runs the real ingest, and reports what it
read from each file. It skips when the corpus is absent, so it never fails on a
machine that has not fetched it.

Its `KNOWN_UNRENDERABLE` list is an allowlist, not a tolerance: a *new* file
that stops rendering fails the test, and so does a listed file that starts
working. The list cannot rot in either direction.

**Eight files in, it found three decode gaps and one display bug** — see
`fixtures/README.md`. Every one of them passed the other 121 tests.

## Invariants worth knowing about

Some tests exist to protect a property that is easy to break by accident and
expensive to discover later.

| Test | What it stops |
|---|---|
| `a_heif_without_a_decoder_is_never_tombstoned` | A tombstone is keyed on the file's mtime, and installing a decoder changes no file's mtime. One written for "no decoder yet" would leave that photo blank **forever** after the upgrade. |
| `the_image_list_never_holds_a_heif` (`db::sql`) | `image_exts` governs the sibling *borrow*. A HEIF in it would render a RAW's thumbnail through a subprocess instead of the RAW's own embedded JPEG. |
| `a_companion_raw_outranks_a_heif` | `primary_rank` is persisted in `groups.primary_file_id`, which travels with the project folder. A tie broken by filename would hand a shot to the member that needs a decoder this machine may not have. |
| `a_group_holding_a_committed_delete_still_absorbs_a_latecomer` | `files.group_id` is a foreign key with `foreign_keys = ON`. Dropping a group while a deleted row still points at it fails the scan transaction — and every scan after it. |
| `the_sql_list_matches_…` / `decodable_photo` | Extension lists that must not drift from what the decoder actually handles. |

## Hooks that need a file you supply

Neither can run in CI: nothing in the repo can produce the input, and checking a
camera file in has provenance questions.

```sh
# One real HEIF, through the whole ladder. Reports capture time, dimensions and
# the rendered mean colour — compare that against the same file decoded by
# something else, because a swapped channel still produces a plausible JPEG.
CULLANT_HEIF_FIXTURE=/path/to/IMG_1234.HEIC cargo test a_real_heif -- --nocapture
```

A `.HEIC` straight off a phone is the single most useful file to point it at. It
also settles whether `kamadak-exif` refusing an EXIF block over 64 KB — which an
iPhone's maker note can exceed — is a real problem or a theoretical one.

## Platform checks

The host build compiles neither the Android Rust nor the Kotlin. Both need
asking for explicitly.

```sh
# Android Rust (store/saf.rs, the SAF plugin's mobile.rs)
export ANDROID_NDK_HOME="C:\Android\sdk\ndk\27.2.12479018"
NDK_BIN="C:/Android/sdk/ndk/27.2.12479018/toolchains/llvm/prebuilt/windows-x86_64/bin"
export PATH="$NDK_BIN:$PATH" \
  CC_aarch64_linux_android="$NDK_BIN/aarch64-linux-android24-clang.cmd" \
  AR_aarch64_linux_android="$NDK_BIN/llvm-ar.exe" \
  CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$NDK_BIN/aarch64-linux-android24-clang.cmd"
cargo check --target aarch64-linux-android

# The plugin's Kotlin
cd src-tauri/gen/android
export JAVA_HOME="C:\Program Files\Microsoft\jdk-17.0.19.10-hotspot" ANDROID_HOME="C:\Android\sdk"
./gradlew :tauri-plugin-saf:compileDebugKotlin
```

See [building.md](building.md) for the full environment.

## Driving the running app

`npm run tauri:debug` starts the app with `--remote-debugging-port=9222`, so any
Chrome DevTools Protocol client can inspect and drive the real window:

```sh
npx @playwright/mcp@latest --cdp-endpoint http://127.0.0.1:9222
```

That gives an accessibility snapshot, clicks, key presses, screenshots, console
errors and the network log of a live session. Use it for UX and discoverability
passes. It is not a substitute for `cargo test`, and `cargo test` is not a
substitute for it — a dozen real UX defects were found this way and none of them
were visible to the suite.

## What is not tested

Stated rather than implied, because the gaps are the useful part.

- **No frontend tests at all.** The stores are rune classes with real logic in
  them (`session.svelte.ts` alone holds the selection model, filters and the
  group view) and none of it is covered. `vitest` is the obvious answer.
- **No pixel decode on any platform but this one.** The HEIF ladder's Android
  and iOS rungs, and WIC on a machine without Microsoft's HEVC extensions, can
  only be checked by hand on that hardware.
- **No video corpus.** `decode/video.rs` has no real files behind it.
- **No RAW+JPEG pair in the corpus.** The `ORF`+`ORI` pair covers N-ary
  grouping; the mirror mode most cameras actually produce is still only tested
  against `touch`ed stand-ins.
- **Benchmarks are not same-session A/B.** Each release build costs ~10 minutes,
  so the recorded numbers come from different runs and are indicative only.
