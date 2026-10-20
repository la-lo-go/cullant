// Build a debug APK for arm64, install it as "Cullant Dev" and start it.
// For tests on a device only. Use `tauri android build` for a release.
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const ANDROID = "src-tauri/gen/android";
const APK = `${ANDROID}/app/build/outputs/apk/universal/debug/app-universal-debug.apk`;
const PACKAGE = "org.cullant.app.dev";

const fail = (message) => {
  console.error(`android:dev: ${message}`);
  process.exit(1);
};

const firstExisting = (paths) => paths.find((path) => path && existsSync(path));

const newestChild = (dir, prefix = "") =>
  existsSync(dir)
    ? readdirSync(dir)
        .filter((name) => name.startsWith(prefix))
        .sort((a, b) => a.localeCompare(b, undefined, { numeric: true }))
        .map((name) => join(dir, name))
        .pop()
    : undefined;

// The tool shells do not always inherit the user's variables, so fall back to
// the paths that Android Studio and Gradle record on this machine.
function sdkFromLocalProperties() {
  const file = `${ANDROID}/local.properties`;
  if (!existsSync(file)) return undefined;
  const line = readFileSync(file, "utf8").match(/^sdk\.dir=(.*)$/m);
  return line?.[1].trim().replace(/\\(.)/g, "$1");
}

const sdk = firstExisting([process.env.ANDROID_HOME, process.env.ANDROID_SDK_ROOT, sdkFromLocalProperties()]);
if (!sdk) fail("cannot find the Android SDK. Set ANDROID_HOME.");

const ndk = firstExisting([process.env.ANDROID_NDK_HOME, newestChild(join(sdk, "ndk"))]);
if (!ndk) fail("cannot find the Android NDK. Set ANDROID_NDK_HOME.");

const java = firstExisting([
  process.env.JAVA_HOME,
  newestChild("C:\\Program Files\\Microsoft", "jdk-"),
  "C:\\Program Files\\Android\\Android Studio\\jbr",
]);
if (!java) fail("cannot find a JDK. Set JAVA_HOME.");

const env = {
  ...process.env,
  ANDROID_HOME: sdk,
  ANDROID_SDK_ROOT: sdk,
  ANDROID_NDK_HOME: ndk,
  JAVA_HOME: java,
};
const adb = join(sdk, "platform-tools", "adb");

// Only npx needs a shell on Windows, because it is a .cmd file. Its arguments
// are quoted, since a shell joins them without escaping.
function run(command, args, shell = false) {
  const options = { stdio: "inherit", env, shell };
  const result = shell
    ? spawnSync([command, ...args.map((arg) => `"${arg}"`)].join(" "), options)
    : spawnSync(command, args, options);
  if (result.status !== 0) fail(`${command} ${args.join(" ")} failed`);
}

// The frontend build takes about a minute. Skip it when no frontend input is
// newer than the last output, which is the case for a Rust-only change.
const FRONTEND_INPUTS = ["src", "static", "package.json", "svelte.config.js", "vite.config.js"];
const newestMtime = (path) =>
  statSync(path).isDirectory()
    ? Math.max(0, ...readdirSync(path).map((name) => newestMtime(join(path, name))))
    : statSync(path).mtimeMs;
const frontendIsCurrent =
  existsSync("build/index.html") &&
  statSync("build/index.html").mtimeMs > Math.max(...FRONTEND_INPUTS.map(newestMtime));

const started = Date.now();
const build = ["tauri", "android", "build", "--debug", "--apk", "--target", "aarch64"];
if (frontendIsCurrent) {
  console.log("android:dev: the frontend has not changed, so its build is skipped");
  const config = join(tmpdir(), "cullant-android-dev.conf.json");
  writeFileSync(config, JSON.stringify({ build: { beforeBuildCommand: "" } }));
  build.push("--config", config);
}
run("npx", build, process.platform === "win32");
if (!existsSync(APK)) fail(`the build did not produce ${APK}`);
run(adb, ["install", "-r", APK]);
run(adb, ["shell", "monkey", "-p", PACKAGE, "-c", "android.intent.category.LAUNCHER", "1"]);
console.log(`android:dev: done in ${Math.round((Date.now() - started) / 1000)} s`);
