// Download the test corpus listed in manifest.json.
//
// The media is deliberately NOT in git: a corpus that covers every RAW format
// is a few hundred megabytes, and git keeps every byte forever. The manifest
// carries a SHA-256 per file instead, so the corpus is reproducible without
// living in the history.
//
//   npm run fixtures            # the small tiers (group + brand), ~90 MB
//   npm run fixtures -- --all   # everything in the manifest, ~350 MB
//   npm run fixtures -- --tier core
//   npm run fixtures -- --max-mb 8
//
// Re-running is cheap: a file whose checksum already matches is left alone.

import { createHash } from "node:crypto";
import { mkdir, readFile, readdir, stat, writeFile } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const mediaDir = join(here, "media");

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? (args[i + 1] ?? true) : undefined;
};

const all = args.includes("--all");
const tier = flag("tier");
const maxMbValue = flag("max-mb");
const maxMb = maxMbValue === undefined ? undefined : Number(maxMbValue);
if (maxMbValue !== undefined && (typeof maxMbValue !== "string" || !Number.isFinite(maxMb) || maxMb <= 0)) {
  throw new Error("--max-mb must be a positive finite number.");
}

async function sha256(path) {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(path)) hash.update(chunk);
  return hash.digest("hex");
}

async function alreadyGood(path, expected) {
  try {
    await stat(path);
  } catch {
    return false;
  }
  return (await sha256(path)) === expected;
}

const manifest = JSON.parse(await readFile(join(here, "manifest.json"), "utf8"));

let wanted = manifest.files;
if (!all && !tier && maxMb === undefined) {
  // The default is the corpus you want on a laptop: the grouping pair, and one
  // file per brand. The per-format sweep is the expensive part and is opt-in.
  wanted = wanted.filter((f) => f.tier !== "core");
}
if (tier) wanted = wanted.filter((f) => f.tier === tier);
if (maxMb !== undefined) wanted = wanted.filter((f) => f.size_mb <= maxMb);

if (wanted.length === 0) {
  console.log("nothing selected; try --all");
  process.exit(0);
}

const total = wanted.reduce((n, f) => n + f.size_mb, 0);
console.log(`${wanted.length} files, ${total.toFixed(0)} MB -> ${mediaDir}`);
await mkdir(mediaDir, { recursive: true });

let fetched = 0;
let skipped = 0;
let failed = 0;

for (const f of wanted) {
  // `local`, never `file`: the upstream names contain ':' (as in "16bit (4:3)"),
  // which on Windows opens an NTFS alternate data stream instead of naming a
  // file — the write appears to succeed and leaves an empty one behind.
  const dest = join(mediaDir, f.local ?? f.file);
  if (await alreadyGood(dest, f.sha256)) {
    skipped++;
    continue;
  }
  process.stdout.write(`  ${f.local ?? f.file} (${f.size_mb.toFixed(1)} MB) ... `);
  try {
    const res = await fetch(f.url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const body = Buffer.from(await res.arrayBuffer());
    // Verify before writing, so a truncated or substituted download never
    // reaches the corpus, where it would look like a decoder bug later.
    const got = createHash("sha256").update(body).digest("hex");
    if (got !== f.sha256) throw new Error(`checksum ${got.slice(0, 12)} != ${f.sha256.slice(0, 12)}`);
    await writeFile(dest, body);
    // And again after writing, because the filesystem gets a say too.
    if (!(await alreadyGood(dest, f.sha256))) throw new Error("write did not land");
    console.log("ok");
    fetched++;
  } catch (err) {
    console.log(`FAILED: ${err.message}`);
    failed++;
  }
}

const have = (await readdir(mediaDir).catch(() => [])).filter((n) => !n.startsWith("."));
console.log(`\nfetched ${fetched}, already had ${skipped}, failed ${failed}`);
console.log(`corpus now holds ${have.length} files`);
if (failed > 0) process.exitCode = 1;
