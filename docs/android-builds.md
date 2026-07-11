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
npx tauri android init        # one-time scaffold (already done — gen/android is committed)
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
10000-day validity, self-signed — fine for sideloading, would need a
Play-App-Signing-compatible key if this ever ships to the Play Store).
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
storeFile=C:\\Users\\<user>\\.android\\cullant-release.jks
storePassword=<password>
keyAlias=cullant
keyPassword=<password>
```

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
