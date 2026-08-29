import { readFileSync, writeFileSync } from "node:fs";

const requested = process.argv[2];
const semver = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/;

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const writeJson = (path, value) =>
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`);

const replaceRequired = (source, pattern, replacement, path) => {
  if (!pattern.test(source)) {
    throw new Error(`Cannot find the application version in ${path}`);
  }
  pattern.lastIndex = 0;
  return source.replace(pattern, replacement);
};

const packageJson = readJson("package.json");
const packageLock = readJson("package-lock.json");
const tauriConfig = readJson("src-tauri/tauri.conf.json");
let cargoToml = readFileSync("src-tauri/Cargo.toml", "utf8");
let cargoLock = readFileSync("src-tauri/Cargo.lock", "utf8");

if (requested) {
  if (!semver.test(requested)) {
    throw new Error(`Invalid semantic version: ${requested}`);
  }
  if (!packageLock.packages?.[""]) {
    throw new Error("Cannot find the root package in package-lock.json");
  }

  const nextCargoToml = replaceRequired(
    cargoToml,
    /(\[package\][\s\S]*?\nversion\s*=\s*")[^"]+("\r?\n)/,
    `$1${requested}$2`,
    "src-tauri/Cargo.toml",
  );
  const nextCargoLock = replaceRequired(
    cargoLock,
    /(\[\[package\]\]\r?\nname = "cullant"\r?\nversion = ")[^"]+("\r?\n)/,
    `$1${requested}$2`,
    "src-tauri/Cargo.lock",
  );

  // Prepare and validate every representation before writing any file. A
  // changed Cargo layout must not leave only the npm files bumped.
  packageJson.version = requested;
  packageLock.version = requested;
  packageLock.packages[""].version = requested;
  cargoToml = nextCargoToml;
  cargoLock = nextCargoLock;
  writeJson("package.json", packageJson);
  writeJson("package-lock.json", packageLock);
  writeFileSync("src-tauri/Cargo.toml", cargoToml);
  writeFileSync("src-tauri/Cargo.lock", cargoLock);
}

const version = requested ?? packageJson.version;
const cargoVersion = cargoToml.match(/\[package\][\s\S]*?\nversion\s*=\s*"([^"]+)"/)?.[1];
const cargoLockVersion = cargoLock.match(
  /\[\[package\]\]\r?\nname = "cullant"\r?\nversion = "([^"]+)"/,
)?.[1];
const mismatches = [
  ["src-tauri/Cargo.toml", cargoVersion],
  ["src-tauri/Cargo.lock", cargoLockVersion],
  ["package-lock.json", packageLock.version],
  ["package-lock.json root package", packageLock.packages[""].version],
].filter(([, value]) => value !== version);

if (tauriConfig.version !== "../package.json") {
  mismatches.push(["src-tauri/tauri.conf.json source", tauriConfig.version]);
}
if (mismatches.length > 0) {
  const details = mismatches.map(([file, value]) => `${file}: ${value}`).join("\n");
  throw new Error(`Version ${version} is not synchronized:\n${details}`);
}

console.log(`Cullant version ${version} is synchronized.`);
