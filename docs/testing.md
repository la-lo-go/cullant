# Testing Cullant

The backend has 122 tests. The frontend has none.

This document tells you what each layer of test data proves, what it cannot
prove, and which checks you must run by hand.

## Checks before a commit

```sh
cd src-tauri
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test

cd ..
npm run check          # must end 0 errors, 0 warnings
```

Also run the app for any change with runtime surface. A green suite is not
enough. The three decoder gaps in section 3 below all passed CI.

## Three kinds of test data

### 1. Inputs built inside the test

Most of the 122 tests use this. Each one makes a `tempfile::tempdir()`, writes a
few files with `touch`, and then asserts on the database.

Use it for:

- grouping, ordering and culling state
- the action queue and the commit step
- XMP round-trips
- SQL predicates

Do not use it for pixels or metadata. `touch(root, "IMG_1.CR3")` writes the four
bytes `raw`. Those bytes stand in correctly for a file the app must count, group
and delete. They stand in for nothing at all when the app must decode the file.

### 2. Synthetic images

```sh
cd src-tauri && cargo run --release --example gen_testdata -- C:\dev\cullant-testdata 600
```

`bench::jpeg_with_exif` in `src-tauri/src/lib.rs` builds a real JPEG with a real
EXIF block. It writes that block with the same library the app reads it with, so
the round-trip always agrees. `bench::heif_with_exif` builds a HEIF container the
same way — `ftyp`, `meta`, `iloc`, `iinf`/`infe` and `idat`, with EXIF inside and
no HEVC at all.

Use it for:

- the metadata pipeline, from scan to database
- ingest order and thumbnail counts
- cache behaviour
- sort and filter facets

Do not use it for the decoder. A synthetic JPEG has none of the four things that
break real decoders:

- maker notes
- an embedded preview at a vendor's private offset
- an `irot` property that disagrees with the EXIF orientation
- a sibling file from the same shot

### 3. Real camera files

```sh
npm run fixtures                          # ~90 MB of CC0 samples
cd src-tauri && cargo test the_corpus -- --nocapture
```

Read [`fixtures/README.md`](../fixtures/README.md) first.

The fetcher downloads this corpus. Git does not store it. A corpus that covers
every RAW format holds a few hundred megabytes, and git keeps every byte
forever. Git tracks `manifest.json` instead, which holds one SHA-256 per file.
The fetcher checks that hash before it writes the file and again afterwards.

The test `the_corpus_of_real_files_is_read_and_rendered` does three things. It
copies every file from `fixtures/media/` into a temporary project. It runs the
real ingest. It then prints what the app read from each file.

The test skips itself when the corpus is absent. It therefore never fails on a
machine that did not fetch it.

`KNOWN_UNRENDERABLE` inside that test is an allowlist. It fails the test in both
directions:

- A file that is not on the list, and stops rendering, fails the test.
- A file that is on the list, and starts rendering, also fails the test.

The second rule keeps the list accurate. Nobody can leave a fixed entry on it.

The first eight files found three decoder gaps and one display error.
`fixtures/README.md` lists them. All four passed the other 121 tests.

## Invariants that tests protect

Each test below guards one property. Each property is easy to break by accident
and expensive to find later.

| Test | What it prevents |
|---|---|
| `a_heif_without_a_decoder_is_never_tombstoned` | The app keys a tombstone on the file's mtime. Installing a decoder changes no file's mtime. A tombstone written for "no decoder yet" therefore keeps that photo blank **forever** after the upgrade. |
| `the_image_list_never_holds_a_heif` (`db::sql`) | `image_exts` controls the sibling borrow. A HEIF inside it would render a RAW's thumbnail through a subprocess, instead of through the RAW's own embedded JPEG. |
| `a_companion_raw_outranks_a_heif` | The database stores `primary_rank` in `groups.primary_file_id`, which travels with the project folder. A tie that the filename breaks could give the shot to a member that needs a decoder this machine does not have. |
| `a_group_holding_a_committed_delete_still_absorbs_a_latecomer` | `files.group_id` is a foreign key, and `foreign_keys` is `ON`. Deleting a group while a deleted row still points at it fails the scan transaction, and every scan after it. |
| `the_sql_list_matches_…` / `decodable_photo` | Two extension lists must agree with what the decoder handles. |

## The hook that needs a file you supply

CI cannot run this test. The repository cannot make the input file, and a camera
file from an unknown source has no clear licence.

```sh
CULLANT_HEIF_FIXTURE=/path/to/IMG_1234.HEIC cargo test a_real_heif -- --nocapture
```

The test runs one real HEIF through the whole decoder ladder. It prints the
capture time, the dimensions and the mean colour of the rendered thumbnail.
Compare that mean colour against the same file decoded by another program: a
swapped colour channel still produces a JPEG that looks correct.

Point it at a `.HEIC` from a phone. That file also answers one open question:
`kamadak-exif` refuses an EXIF block larger than 64 KB, and an iPhone maker note
can exceed that size.

## Platform checks

The host build compiles neither the Android Rust nor the Kotlin. Ask for each
one directly.

```sh
# Android Rust: store/saf.rs, and the SAF plugin's mobile.rs
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

[building.md](building.md) describes the full environment.

## How to drive the running app

`npm run tauri:debug` starts the app with `--remote-debugging-port=9222`. Any
Chrome DevTools Protocol client can then inspect and control the real window.

```sh
npx @playwright/mcp@latest --cdp-endpoint http://127.0.0.1:9222
```

The client can then do all of this against a live session:

- read the accessibility tree
- send clicks and key presses
- take screenshots
- read the console errors and the network log

Use it for UX and discoverability passes. It does not replace `cargo test`, and
`cargo test` does not replace it. One pass over the running app found twelve real
UX defects. The suite showed none of them.

## What the tests do not cover

This list is explicit, because the gaps matter more than the coverage.

- **The frontend has no tests.** The rune-class stores hold real logic.
  `session.svelte.ts` alone holds the selection model, the filters and the group
  view. `vitest` is the obvious answer.
- **No machine but this one decodes pixels under test.** You must check the
  Android and iOS rungs of the HEIF ladder by hand, on that hardware. You must
  also check WIC on a Windows machine that lacks Microsoft's HEVC extensions.
- **No video corpus exists.** No real file backs `decode/video.rs`.
- **The corpus holds no RAW+JPEG pair.** The `ORF`+`ORI` pair covers N-ary
  grouping. Mirror mode, which most cameras produce, still runs only against
  files written with `touch`.
- **The benchmarks do not compare two builds in one session.** Each release
  build costs about 10 minutes, so the recorded numbers come from separate runs.
  Treat them as indications, not measurements.
