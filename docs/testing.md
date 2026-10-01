# Testing Cullant

For first-import orientation, see the [deterministic regression and app checks](first-image-orientation.md#repeatable-checks).

## Measure photo preparation on a real disk

Use a release build. Keep the source project and generated cache on the disk under test.
The commands below create a new root project. They preserve existing projects in subfolders.
Do not use a root that already contains `.cullant` for the first run.

1. Build the frontend with `npm run build`.
2. Copy the debug Tauri configuration into the local artifact directory.
3. Set its identifier to `org.cullant.performance` and its CDP port to `9223`.
4. Set `TAURI_CONFIG` to that configuration's JSON text. Build with `cargo build --release --manifest-path src-tauri/Cargo.toml --bin cullant`.
5. For this direct Cargo build, serve production assets with `npm run preview -- --host 127.0.0.1 --port 1420 --strictPort`.
6. Record source hashes with `python scripts/snapshot-photo-project.py D:/ artifacts/performance/2026-10-01-d-drive/before.json`.
7. Run the following PowerShell commands from the repository root.

```powershell
./scripts/profile-photos.ps1 -Cache first -Run first-2560
./scripts/profile-photos.ps1 -Cache warm -Run warm-2560
./scripts/profile-photos.ps1 -Cache reset -Run repeat-2560-a -QuietNavigation
./scripts/profile-photos.ps1 -Cache reset -Run repeat-2560-b -QuietNavigation
./scripts/profile-photos.ps1 -Cache reset -Run size-1600 -Edge 1600 -QuietNavigation
./scripts/profile-photos.ps1 -Cache reset -Run size-3840 -Edge 3840 -QuietNavigation
./scripts/profile-photos.ps1 -Cache reset -Run disabled-2560 -Disabled -QuietNavigation
./scripts/profile-photos.ps1 -Cache reset -Run final-2560 -QuietNavigation
./scripts/profile-photos.ps1 -Cache warm -Run warm-final-2560 -MeasureNavigation
./scripts/profile-photos.ps1 -Cache reset -Run navigation-2560 -MeasureNavigation
```

The runner opens the folder through the real UI. Select D: in the native picker on the first run.
Later runs click the saved project card. The Tauri IPC function is immutable; do not replace it to intercept the picker.
The first backend timestamp excludes time spent waiting in the picker. Retain raw timestamps for audit.
It checks generated cache files and records screenshots, phase events, resource samples, and navigation observations.
Only `thumbnails` rows and their generated photo cache files are reset. Project state is retained.
Reset requires an ownership file from the first run and a matching database root.
Close the measured app before a reset. The runner stops only the application process that it starts.

The separate application identifier isolates test preferences from normal settings.
Video poster generation is disabled. Video metadata can still contribute to the metadata phase.
Source hash reads warm the Windows file cache. Label these runs as empty application cache, not cold disk cache.

Run `node scripts/check-photo-profile.mjs <run-profile.jsonl>` for each instrumented generation run.
Repeat the source snapshot after measurement. Compare source hashes and existing subproject cache metadata.
Run `python scripts/verify-photo-cache.py D:/ <result.json>` to decode all final cache images and check their orientation keys.
This verification script requires Pillow. The final cache must use 2560-pixel previews.
Run `node scripts/analyze-photo-profile.mjs <artifact-directory>` to summarize stage and resource measurements.
Store raw artifacts locally. Summarize results and limitations in `hardware-acceleration.md`.
Do not add nested stage durations together. See that report for counter definitions.

Use end-to-end (E2E) tests for app behavior. Use public engine tests when an app test cannot reach a failure safely.

Write the regression before you change source code. List the failure modes before you test a system in isolation.
Prefer the real app for complex features. Keep a repeatable artifact, such as a result file, screenshot, or disposable project.

This document tells you what each layer of test data proves, what it cannot
prove, and which checks you must run by hand.

## Checks before a commit

```sh
cd src-tauri
cargo fmt --check
cargo clippy --all-targets -- -W clippy::cognitive_complexity -D warnings
cargo test

cd ..
npm run check          # must end 0 errors, 0 warnings
```

Also run the app for any change with runtime surface. A green suite is not
enough. The three decoder gaps in section 3 below all passed CI.

## Three kinds of test data

### 1. Inputs built inside the test

Backend tests can make a `tempfile::tempdir()`, write files, and check the public engine result and database state.

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
`fixtures/README.md` lists them. The other backend checks did not find these failures.

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

## Folder and action controls

Create the disposable project. Then start the debug app with this project.

```powershell
node scripts/e2e-folder-ux.mjs --prepare
$env:CULLANT_OPEN_PROJECT = (Resolve-Path .playwright-mcp/folder-ux-project).Path
npm run tauri:debug
```

Run the checks in a second terminal at the repository root.

```powershell
node scripts/e2e-folder-ux.mjs .playwright-mcp/folder-ux-green
```

The script uses the real Windows app through port 9222. It checks mouse, keyboard, and touch controls.
It also checks folder exclusions, project isolation, collapsed branches, and a missing preview after a rescan.
It checks the All context menu, folder menu feedback, Compare zoom focus, and touch bar icons.
It checks Ctrl and Shift folder selection and saved folder scope.
The first zoom checks use wheel, double tap, and keyboard input. They also check a change in source dimensions.
It saves `results.json` and screenshots in the output folder. It restores the bar layouts and app preferences.
Touch checks use pointer emulation. Run Android device checks separately.
Set `CULLANT_CDP_PORT` if the debug app uses a port other than 9222.

## Preview cache recovery

Use Node 24 for the E2E scripts. They use built-in SQLite and WebSocket support.
Create the disposable project. Then start the debug app with this project.

```powershell
node scripts/e2e-preview-cache.mjs --prepare
$env:CULLANT_OPEN_PROJECT = (Resolve-Path .playwright-mcp/preview-cache-project).Path
npm run tauri:debug
```

Run the checks in a second terminal at the repository root.

```powershell
node scripts/e2e-preview-cache.mjs --output=.playwright-mcp/preview-cache
```

The script uses the real app through port 9222. It changes only the disposable project.
It checks cached previews, missing cache files, damaged JPEG files, readiness updates, and source access failures.
It also checks thumbnail spinners and the first image in the loupe.
It checks old failure records and visible group members in separate mode.
It saves `results.json` and screenshots in the output folder. It restores app preferences.

These fixtures test cache and UI behavior. They do not test RAW decoding.
Cache validation checks the JPEG header and end marker. It does not decode every cached pixel.

The same cache code runs on desktop and Android. Run Android device checks separately.

## Date filters and Settings input

Start the debug app with the disposable project from the preview cache checks.
Run this command in a second terminal at the repository root.

```powershell
node scripts/e2e-ui-polish.mjs .playwright-mcp/ui-polish
```

The script checks date presets at UTC day boundaries and folder branch actions.
It also checks hover colors, label name fields, touch input, and keyboard focus.
It uses temporary catalog rows in the app. It does not change source files or project rows.
It saves `results.json` and screenshots. It restores preferences and reloads the app.
Touch checks use WebView pointer emulation. Run Android device checks separately.

## Action bar and Compare

Start the debug app with the disposable project from the preview cache checks.
Run this command in a second terminal at the repository root.

```powershell
node scripts/e2e-action-bar.mjs .playwright-mcp/action-bar
```

The script checks saved bar layouts, Auto visibility, and the narrow toolbar.
It checks theme hover feedback for active and inactive bar and view buttons.
It checks the Fit whole photo default and saved framing choices after reload.
It checks the current item marker and photo zoom targets in Compare.
It also checks mouse help, keyboard help, touch holds, short taps, and scrolling.
Video rows test control visibility. They do not test video playback.
It saves `results.json` and screenshots. It restores preferences and reloads the app.
Touch checks use WebView emulation. Run Android device checks separately.

## First video and playback failure

Use Node 24 and ffmpeg. Create the disposable project. Then start the debug app.

```powershell
node scripts/e2e-video-first-open.mjs --prepare
$env:CULLANT_OPEN_PROJECT = (Resolve-Path .playwright-mcp/video-first-open-project).Path
npm run tauri:debug
```

Run the checks in a second terminal at the repository root.

```powershell
node scripts/e2e-video-first-open.mjs .playwright-mcp/video-first-open
```

The script checks MJPEG clips that need an external player and an H.264 clip that plays in the app.
It clears the fixture's preview cache before the first clip opens.
It checks sharp posters, early failure messages, narrow screens, remux playback, and navigation.
It checks that one grid tap keeps playback paused and that a false playback event keeps the poster.
It injects load errors to check poster retries and late decoder failures.
It checks the external action's file ID without starting another application.
It checks that preparation notices wait for compatibility and a valid conversion plan.
It checks that every transport control is disabled after failure.
It checks that confirmed playback failures reopen without new video reads.
It checks file changes and projects with the same file IDs.
It checks that temporary read failures can retry without an app restart.
It checks that the warning has no help action.
It delays the native response to check that an unsupported codec warns without another tap.
It saves `results.json` and screenshots. It restores app preferences.
These synthetic clips do not test real camera videos. Run Android device checks separately.

For the Android check, open a project and its Videos grid on the connected phone.
Use a camera clip that shows the external player warning after a playback attempt.
Leave Cullant in the foreground while the test runs.

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/e2e-android-video.ps1 -Device '100.89.241.106:45353' -VideoName 'DSCF5044.MOV' -Output '.playwright-mcp/android-video'
```

The script opens the clip with one tap. A second tap can hide the original defect.
It compares the initial and settled posters with the poster after the playback attempt.
It checks that the external warning appears before that attempt.
It saves screenshots, UI XML, and `results.json`. It stops if another app takes focus.

With a diagnostic WebView connected to CDP port 9223, test the real camera clip.
The script saves request logs, results, and ADB screenshots.

```powershell
$env:CULLANT_EXPECT_PLAYABLE = '1'
node scripts/e2e-android-video-memory.mjs .playwright-mcp/android-camera-playback DSCF5044.MOV
Remove-Item Env:CULLANT_EXPECT_PLAYABLE
$env:CULLANT_FORCE_UNSUPPORTED = '1'
node scripts/e2e-android-video-memory.mjs .playwright-mcp/android-failure-memory DSCF5044.MOV
Remove-Item Env:CULLANT_FORCE_UNSUPPORTED
```

The first run checks preparation, frames, and seeking with the real codec.
The second run injects an unsupported codec result. It checks the warning and failure memory.
Run the second check in a fresh app session. Restart the app after fault injection.
With the same clip open and paused before Play, compare query range reads with its local copy.
If playback has hidden the poster, set `CULLANT_VIDEO_URL` to the `/video/` URL from the request log.

```powershell
node scripts/e2e-video-range.mjs .playwright-mcp/android-video-ranges .playwright-mcp/android-video-camera/DSCF5044.MOV
```

Each result includes byte counts, Content-Range, and SHA-256 hashes.
Remove WebView debugging before the final APK is built and installed.

## Burst image fit and indicators

Open a real photo project in an isolated debug app with CDP port 9223 and Vite development assets.
Use a project with portrait and landscape bursts. Run:

```powershell
node scripts/e2e-burst-fit.mjs artifacts/performance/burst-counter-desktop
$env:CULLANT_GRID_TEST_WIDTH = '390'
node scripts/e2e-burst-fit.mjs artifacts/performance/burst-counter-mobile
Remove-Item Env:CULLANT_GRID_TEST_WIDTH
```

The script checks fit and fill modes, collapsed and expanded bursts, and three scroll positions.
It checks image proportions, frame bounds, burst counters, and the front-card border.
It saves measurements and screenshots, then restores view preferences.
The D: checks passed at both widths: 12 observations and 102 rear cards per run.
The border comparison also confirmed that all 354 matched image boxes kept their dimensions.
Narrow-window emulation does not replace a physical mobile-device check.

Rear cards now use their own image proportions in fit mode.
Narrow portrait cards wrap the indicators instead of hiding the burst icon and count.
Front and rear outlines are drawn inside the image bounds.

## What the tests do not cover

This list is explicit, because the gaps matter more than the coverage.

- **Type checks do not verify frontend behavior.** Use app E2E checks for selection, filters, groups, and keyboard actions.
- **No machine but this one decodes pixels under test.** You must check the
  Android rung of the HEIF ladder by hand, on that hardware. You must
  also check WIC on a Windows machine that lacks Microsoft's HEVC extensions.
- **No video corpus exists.** No real file backs `decode/video.rs`.
- **Synthetic pairing tests do not decode real RAW+JPEG pairs.** The `ORF`+`ORI` pair covers N-ary
  grouping. The local D: performance experiment adds 170 real pairs and checks generated artifacts.
  Its source photos are not distributed with the repository.
- **The benchmarks do not compare two builds in one session.** Each release
  build costs about 10 minutes, so the recorded numbers come from separate runs.
  Treat them as indications, not measurements.
