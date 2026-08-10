# Building Cullant

Cullant is a Tauri v2 app with two targets: **Windows desktop** (the primary
target) and an **Android** port (`src-tauri/gen/android/`, checked into git).
This doc covers how to build each, the size/target tradeoffs, and how release
signing works for Android.

## Windows desktop

```sh
npm run tauri dev            # dev build, hot reload
npm run tauri build           # release build -> NSIS + MSI installers
CULLANT_OPEN_PROJECT=<dir> npm run tauri dev   # auto-open a project at startup
```

Output: `src-tauri/target/release/bundle/{nsis,msi}/`.
Profile is tuned in `src-tauri/Cargo.toml` (`[profile.release]`: `strip = true`,
`lto = true`, `codegen-units = 1`) — smaller and faster than the Cargo default,
at the cost of build time (LTO + `codegen-units = 1` serialize codegen).

## Android

### Required environment

Building for Android needs three things beyond the base toolchain: the
Android SDK **with `cmdline-tools`** installed, the NDK (for cross-compiling
the Rust side), and a JDK. On this machine there are *two* SDK installs —
only one is complete:

| | Path | Has `cmdline-tools`? | Use it? |
|---|---|---|---|
| Correct | `C:\Android\sdk` | Yes | ✅ |
| Incomplete (Tauri's auto-detected fallback) | `C:\Users\<user>\AppData\Local\Android\Sdk` | No | ❌ fails with "failed to ensure Android environment" |

The correct paths are set as **User**-scope Windows env vars, but a fresh
shell tool session doesn't always inherit them — export explicitly before
building:

```sh
export ANDROID_HOME="C:\Android\sdk"
export ANDROID_SDK_ROOT="C:\Android\sdk"
export ANDROID_NDK_HOME="C:\Android\sdk\ndk\27.2.12479018"
export JAVA_HOME="C:\Program Files\Microsoft\jdk-17.0.19.10-hotspot"
```

### Build commands and what they produce

```sh
npx tauri android init        # one-time scaffold (already done — gen/android is committed;
                              # re-running overwrites customizations, see "Re-running" below)
npx tauri android dev         # dev build, deploys to a connected device/emulator
npx tauri android build [flags]
```

`android build` compiles **release mode by default** — `--debug` is opt-in,
not opt-out. Debug builds are unminified, `isDebuggable`/`isJniDebuggable`,
and keep debug symbols for all 4 ABIs (see `buildTypes.debug` in
`app/build.gradle.kts`), so they're always the largest, slowest option and
should only be used for on-device debugging.

Key flags:

| Flag | Effect |
|---|---|
| `--target <aarch64\|armv7\|i686\|x86_64>` | Build for one or more specific ABIs instead of all four. Repeatable. |
| `--apk` | Emit `.apk` (installable directly / sideload). |
| `--aab` | Emit `.aab` (Play Store upload format; not installable directly). |
| `--split-per-abi` | With multiple targets, emit one APK/AAB per ABI instead of a single fat one. |
| (no target flags) | Builds all 4 ABIs (`aarch64`, `armv7`, `i686`, `x86_64`) into one **universal** artifact bundling every `.so`. |

### Type-checking the Android target without a full build

Anything behind `#[cfg(target_os = "android")]` — the whole SAF store, the
thread-pool caps, the logcat setup — is invisible to `cargo clippy` on Windows,
so it can break without any local check failing. A `cargo check` against the
target catches that in ~20 s instead of a ~10 min build.

`rusqlite` is bundled, so it needs the NDK's C compiler on `PATH`:

```sh
NDK="/c/Android/sdk/ndk/27.2.12479018/toolchains/llvm/prebuilt/windows-x86_64/bin"
export PATH="$NDK:$PATH"
export ANDROID_NDK_HOME="C:\Android\sdk\ndk\27.2.12479018"
export CC_aarch64_linux_android="$NDK/aarch64-linux-android24-clang.cmd"
export AR_aarch64_linux_android="$NDK/llvm-ar.exe"
cd src-tauri && cargo check --target aarch64-linux-android --lib
```

Without the `CC_*`/`PATH` exports it fails at `cc-rs: failed to find tool
"clang.exe"` while building `libsqlite3-sys`, not at anything in this codebase.

### Reading the app's logs on a device

Android discards a native library's stdout, so `tracing_subscriber::fmt()` shows
nothing there. The app installs `android_logger` instead (see `run()` in
`src-tauri/src/lib.rs`), so every `tracing::info!` reaches logcat under one tag:

```sh
adb logcat -s cullant
```

That is where the ingest reports its per-phase timings and its storage counters
(`N sources, X GB, Ys in opens, Z backend calls`) — the numbers that say whether
an import is bound by reading files or by something else.

### Launch (splash) screen

The launch screen is customized: the app theme's `windowBackground` is a
layer-list painting the app background color (`@color/cullant_bg` = `#222728`,
the `bg` theme token from `src/routes/+page.svelte`) with the Cullant mark
centered at 96dp. On Android 12+ the system splash also derives its background
from `windowBackground`, so the same color shows from the very first frame.

Files involved (all under `src-tauri/gen/android/app/src/main/res/`):

- `values/colors.xml` — adds `cullant_bg`.
- `drawable/splash_logo.xml` — the brand mark as a hand-written vector
  drawable (converted from `logo/icon-square.svg`).
- `drawable/splash_background.xml` — layer-list: `cullant_bg` + centered
  `splash_logo`.
- `values/themes.xml` and `values-night/themes.xml` — set
  `android:windowBackground` to `@drawable/splash_background`.

### Re-running `tauri android init`

Re-init **overwrites** scaffold-owned files unconditionally: the generator
(cargo-mobile2's bicycle templating) copies/renders every template file over
whatever exists, with no skip-if-modified logic. Files you created that are
not in the template pack (e.g. the two `drawable/splash_*.xml` above) survive,
but template-owned files are reset to scaffold defaults — including
`values/themes.xml`, `values-night/themes.xml`, `values/colors.xml`,
`layout/activity_main.xml`, `AndroidManifest.xml`, and
`app/build.gradle.kts` (which would also drop the release-signing wiring).

Since `gen/android` is committed, the recovery path after any re-init is:

```sh
git status src-tauri/gen/android          # see what was reset
git checkout -- src-tauri/gen/android     # restore all customizations
```

If the scaffold was regenerated *on purpose* (e.g. after a Tauri CLI upgrade),
diff instead and re-apply only the customizations:

1. `values/colors.xml`: re-add the `cullant_bg` color (`#FF222728`).
2. `values/themes.xml` and `values-night/themes.xml`: add
   `<item name="android:windowBackground">@drawable/splash_background</item>`
   to `Theme.cullant`.
3. `app/build.gradle.kts`: re-apply the release-signing block (see
   "Release signing" below).
4. The `drawable/splash_logo.xml` / `splash_background.xml` files survive
   untouched; restore from git if they were deleted anyway.

### Size comparison (why target matters)

The default no-flag build links all four architectures' native libraries
into one "universal" APK — most of that weight is `.so` duplication for
ABIs that will never run on the device installing it (essentially every real
phone/tablet since ~2017 is `arm64-v8a`; `armeabi-v7a`/`x86`/`x86_64` only
matter for old devices or emulators).

**Smallest non-debug artifact** = release + single ABI:

```sh
npx tauri android build --apk --target aarch64
```

This produced a **19.6 MB** unsigned APK containing only `lib/arm64-v8a/*.so`,
versus a universal build that bundles 4 copies of the native lib (roughly
4x the native-code weight — the JS/asset bundle is shared and doesn't
duplicate).

If distributing to unknown/mixed hardware, either:
- Use `--aab` (Play Store store-side splits: Google serves each device only
  its matching ABI slice), or
- Use `--target aarch64 armv7 --split-per-abi` and ship both APKs, letting
  the installer/store pick.

### Release signing

`app/build.gradle.kts` (tracked in git) is wired to sign the `release`
build type automatically, **if** `src-tauri/gen/android/keystore.properties`
exists — that file is gitignored (never commit it or the `.jks`), so a fresh
checkout without it just produces an unsigned release APK like Tauri's
default scaffold.

The actual keystore lives outside the repo entirely:
`C:\Users\lalop\.android\cullant-release.jks` (alias `cullant`, RSA 2048,
10000-day validity, self-signed). Those properties also satisfy Play App
Signing, which asks for RSA 2048 or more and validity past October 2033 —
see "One key for every channel" below.
`keystore.properties` holds the store/key passwords in plaintext locally —
that's the normal (if imperfect) pattern for local Android release signing;
don't move the passwords into any tracked file.

To (re)generate a keystore from scratch:

```sh
keytool -genkeypair -v \
  -keystore "C:\Users\<user>\.android\cullant-release.jks" \
  -alias cullant -keyalg RSA -keysize 2048 -validity 10000
```

Then create `src-tauri/gen/android/keystore.properties`:

```properties
storeFile=C:/Users/<user>/.android/cullant-release.jks
storePassword=<password>
keyAlias=cullant
keyPassword=<password>
```

**Use forward slashes in `storeFile`.** A `.properties` file treats `\` as an
escape character, so a Windows path with single backslashes
(`C:\Users\…`) is mangled (the separators vanish) and Gradle then resolves it
relative to `app/` and fails with "Keystore file … not found". Forward slashes
are left untouched by the parser and are still absolute on Windows.

Once that file exists, `npx tauri android build --apk --target aarch64`
signs automatically — no manual signing step needed. (The very first signed
build in this repo was produced by manually zipaligning + `apksigner sign`-ing
an already-built unsigned APK, before the gradle wiring existed; that manual
path is no longer necessary.)

To manually sign/verify an existing unsigned APK (e.g. if you don't want to
trigger a full rebuild):

```sh
BT=/c/Android/sdk/build-tools/35.0.0
"$BT/zipalign.exe" -v -p 4 app-unsigned.apk app-aligned.apk
"$BT/apksigner.bat" sign --ks cullant-release.jks --ks-key-alias cullant \
  --out cullant-release.apk app-aligned.apk
"$BT/apksigner.bat" verify -v cullant-release.apk
```

### Signing in CI

`.github/workflows/release.yml` builds the APK on a GitHub runner, which has
no access to the local keystore. The runner rebuilds `keystore.properties`
from four repository secrets, then Gradle signs as it does locally.

| Secret | Value |
|---|---|
| `ANDROID_KEYSTORE_B64` | the `.jks` file, base64-encoded |
| `ANDROID_STORE_PASSWORD` | the store password |
| `ANDROID_KEY_ALIAS` | `cullant` |
| `ANDROID_KEY_PASSWORD` | the key password |

Add them at **Settings → Secrets and variables → Actions → New repository
secret**. To put the base64 text on the clipboard, in PowerShell:

```powershell
[Convert]::ToBase64String([IO.File]::ReadAllBytes("$env:USERPROFILE\.android\cullant-release.jks")) | Set-Clipboard
```

Then paste it into `ANDROID_KEYSTORE_B64`. The text must stay on one line:
a line break makes the decode fail on the runner. `ToBase64String` writes
one line, and so does `base64 -w0` if you prefer Git Bash. Do not write the
text to a file unless you delete the file afterwards.

The workflow stops before the build if `ANDROID_KEYSTORE_B64` is empty, and
stops after the build if no signed APK is present. It then runs `apksigner
verify` on the result, so an unsigned or badly signed APK can never reach a
Release.

**A password that holds a backslash breaks this.** The `.properties` format
reads `\` as an escape character. The local file has the same limit — see
the `storeFile` note above.

### One key for every channel

`cullant-release.jks` is the identity of the app on every channel that is
planned. Android refuses an update whose signature changed, so a user can
only move between channels if all of them sign with this one key.

- **GitHub Releases** signs with it today. This is the only channel now.
- **IzzyOnDroid** republishes the GitHub APK without a rebuild, so it keeps
  this signature. No new key is necessary.
- **Google Play**, later: at Play App Signing enrollment, **upload this key**
  as the app signing key. Do not let Google make one. The choice is made
  once and cannot be reversed.
- **f-droid.org** cannot share it: they build from source and sign with
  their own key. Only a reproducible build gets around that, which Rust
  makes expensive — see the notes on remapped paths, the pinned NDK, and R8.

**Keep a copy of the keystore and of both passwords away from this machine.**
The file has no backup in the repo, by design. After the first user installs
an APK, a lost keystore means every user must uninstall to move on.
