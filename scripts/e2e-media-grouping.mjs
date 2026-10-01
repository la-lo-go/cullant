import { mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { execFileSync } from "node:child_process";

// Failure cases: photo settings leak into videos; unknown or equal metadata
// creates empty controls; filtered groups stay visible; saved groups lose a tab;
// a missing tool blocks culling; OK repeats; suppression hides unrelated tools.
const output = resolve(process.argv[2] ?? ".playwright-mcp/media-grouping");
const fixture = resolve(".playwright-mcp/media-grouping-project");
if (process.argv.includes("--prepare")) {
  await mkdir(fixture, { recursive: true });
  execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=160x120", "-frames:v", "1", join(fixture, "01-photo.png")]);
  for (const [name, size, codec, rate, duration] of [["02-h264.mp4", "320x240", "libx264", "24", "1"], ["03-mjpeg.mov", "240x320", "mjpeg", "60", "11"]]) {
    execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", `testsrc2=size=${size}:rate=${rate}`, "-t", duration, "-an", "-c:v", codec, "-pix_fmt", "yuv420p", "-metadata", "creation_time=2021-02-03T04:05:06Z", join(fixture, name)]);
  }
  console.log(fixture);
  process.exit(0);
}
const targets = await (await fetch(`http://127.0.0.1:${process.env.CULLANT_CDP_PORT ?? 9222}/json/list`)).json();
const target = targets.find(page => page.type === "page" && page.url.includes("localhost:1420"));
if (!target) throw new Error("Start the debug app with a disposable project.");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let sequence = 0;
const pending = new Map(), results = [], observations = [];
socket.onmessage = ({ data }) => {
  const reply = JSON.parse(data), task = pending.get(reply.id);
  if (!task) return;
  pending.delete(reply.id);
  if (reply.error) task.reject(new Error(reply.error.message)); else task.resolve(reply.result);
};
function send(method, params = {}) {
  return new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })); });
}
async function evaluate(fn, arg) {
  const reply = await send("Runtime.evaluate", { expression: `(${fn.toString()})(${JSON.stringify(arg ?? null)})`, awaitPromise: true, returnByValue: true });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result.value;
}
async function settle() { await evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))); }
async function waitFor(fn) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (await evaluate(fn).catch(() => false)) return;
    await new Promise(resolve => setTimeout(resolve, 50));
  }
  throw new Error(`Timed out: ${fn.toString()}`);
}
function assert(condition, message) { if (!condition) throw new Error(message); }
async function check(name, run) {
  try { await run(); results.push({ name, status: "PASS" }); } catch (error) { results.push({ name, status: "FAIL", error: error.message }); }
  console.log(`${results.at(-1).status}: ${name}`);
}
await mkdir(output, { recursive: true });
let original;
let originalRemember;
try {
  await send("Page.reload");
  await waitFor(() => performance.getEntriesByType("resource").some(entry => new URL(entry.name).pathname === "/src/lib/stores/catalog.svelte.ts"));
  await new Promise(resolve => setTimeout(resolve, 300));
  await evaluate(async () => {
    const loaded = path => import(performance.getEntriesByType("resource").find(entry => new URL(entry.name).pathname === path)?.name ?? path);
    const { catalog } = await loaded("/src/lib/stores/catalog.svelte.ts");
    const { session } = await loaded("/src/lib/stores/session.svelte.ts");
    const { settings } = await loaded("/src/lib/stores/settings.svelte.ts");
    const { view } = await loaded("/src/lib/stores/view.svelte.ts");
    const { api } = await loaded("/src/lib/api.ts");
    const { runtimeTools } = await loaded("/src/lib/stores/runtimeTools.svelte.ts");
    const { folders } = await loaded("/src/lib/stores/folders.svelte.ts");
    window.mg = { catalog, session, settings, view, api, runtimeTools, folders };
  });
  originalRemember = await evaluate(() => mg.settings.rememberSession);
  original = await evaluate(() => ({ root: mg.catalog.project?.rootPath, items: mg.catalog.items, media: mg.catalog.media, groups: mg.session.sessionSnapshot(), remember: mg.settings.rememberSession, scope: { ...mg.folders.scope }, storage: { ...localStorage } }));
  if (process.argv.includes("--verify-scan")) {
    await check("Real video properties reach the catalog through the metadata pass", async () => {
      await evaluate(async fixture => { mg.settings.setRememberSession(false); await mg.catalog.open(fixture); }, fixture);
      await waitFor(() => mg.catalog.mediaCounts.photos === 1 && mg.catalog.mediaCounts.videos === 2 && !mg.catalog.scanning && mg.catalog.metaProgress.total === 0);
      await evaluate(async () => { await mg.catalog.setMedia("videos"); mg.session.viewPanelOpen = true; });
      await waitFor(() => mg.catalog.items.every(item => item.videoCodec && item.videoFrameRate && item.videoDuration));
      const rows = await evaluate(() => mg.catalog.items.map(item => ({ name: item.name, codec: item.videoCodec, fps: item.videoFrameRate, duration: item.videoDuration, width: item.width, height: item.height, capture: item.captureTime })));
      const tools = await evaluate(() => mg.api.runtimeTools());
      observations.push({ case: "real-video-metadata", rows, tools });
      assert(tools.ffmpegPresent && tools.ffprobePresent && tools.hasVideos, "The runtime probe did not find the installed video tools");
      const h264 = rows.find(row => row.codec === "h264"), mjpeg = rows.find(row => row.codec === "mjpeg");
      assert(h264?.width === 320 && h264.height === 240 && h264.fps === 24 && h264.duration === 1, "H264 metadata was not read");
      assert(mjpeg?.width === 240 && mjpeg.height === 320 && mjpeg.fps === 60 && mjpeg.duration === 11, "MJPEG metadata was not read");
      assert(rows.every(row => row.capture === Date.parse("2021-02-03T04:05:06Z") / 1000), "Video grouping uses the wrong date");
      const dims = await evaluate(() => mg.session.groupDims.map(dim => dim.key));
      assert(["codec", "resolution", "frameRate", "duration", "orientation", "ext"].every(key => dims.includes(key)), "Real video grouping properties are missing");
      const shot = await send("Page.captureScreenshot", { format: "png" });
      await writeFile(join(output, "real-video-groups.png"), Buffer.from(shot.data, "base64"));
    });
    await check("Real tab changes and project reopen preserve both grouping choices", async () => {
      await evaluate(async () => {
        mg.session.groupBy = ["codec"];
        await mg.catalog.setMedia("photos");
        mg.session.groupBy = ["date"];
        await mg.catalog.setMedia("videos");
      });
      assert(await evaluate(() => mg.session.groupBy.join() === "codec"), "The real tab switch lost video groups");
      await evaluate(async fixture => {
        await mg.api.setSessionState(JSON.stringify(mg.session.sessionSnapshot()));
        mg.settings.setRememberSession(true);
        await mg.catalog.open(fixture);
      }, fixture);
      await waitFor(() => !mg.session.restoring && mg.catalog.items.length === 2);
      assert(await evaluate(() => mg.catalog.media === "videos" && mg.session.groupBy.join() === "codec"), "The project reopen lost video groups");
      await evaluate(() => mg.catalog.setMedia("photos"));
      assert(await evaluate(() => mg.session.groupBy.join() === "date"), "The project reopen lost photo groups");
    });
  }
  await evaluate(() => { mg.settings.setRememberSession(false); mg.session.resetForNewProject(); mg.folders.selectOnly(null); mg.view.mode = "grid"; localStorage.removeItem("cullant.hiddenRuntimeTools"); });
  await check("Photo and video grouping choices are independent", async () => {
    await evaluate(() => { mg.catalog.media = "photos"; mg.session.groupBy = ["camera"]; mg.catalog.media = "videos"; });
    assert(await evaluate(() => mg.session.groupBy.length === 0), "Videos inherited Camera");
    await evaluate(() => { mg.session.groupBy = ["codec"]; mg.catalog.media = "photos"; });
    assert(await evaluate(() => mg.session.groupBy.join() === "camera"), "Photos lost Camera");
    assert(await evaluate(() => mg.session.sessionSnapshot().groupByMedia.videos.join() === "codec"), "The video choice is not saved");
  });
  await check("Video dimensions use video metadata and hide photo dimensions", async () => {
    await evaluate(() => {
      const base = mg.catalog.items[0];
      mg.catalog.media = "videos";
      mg.catalog.items = ["h264", "hevc"].map((codec, index) => ({ ...base, id: index + 1, groupId: index + 1, kind: 2, name: `${index}.mp4`, relPath: `${index}.mp4`, ext: "mp4", rating: 0, flag: 0, label: null, groupSize: 1, captureTime: 1700000000, mtime: 1700000000, width: 1920, height: 1080, orientation: 1, camera: `Camera ${index}`, videoCodec: codec, videoDuration: index ? 120 : 5, videoFrameRate: index ? 60 : 24 }));
      mg.session.clearFilters(); mg.session.viewPanelOpen = true;
    });
    await settle();
    const dims = await evaluate(() => mg.session.groupDims.map(dim => dim.key));
    assert(["codec", "frameRate", "duration"].every(key => dims.includes(key)), "Video properties are not offered");
    assert(!dims.some(key => ["camera", "lens", "burst", "type", "iso"].includes(key)), "Photo dimensions leaked into videos");
    assert(await evaluate(() => !!document.querySelector('[role="dialog"][aria-label="View"]')), "The View panel did not open");
    assert(await evaluate(() => ["codec", "frameRate", "duration"].every(key => document.querySelector(`[aria-label="View"] option[value="${key}"]`))), "The video grouping controls are missing from the panel");
    await new Promise(resolve => setTimeout(resolve, 100));
    const shot = await send("Page.captureScreenshot", { format: "png" });
    await writeFile(join(output, "video-groups.png"), Buffer.from(shot.data, "base64"));
  });
  await check("Equal, null and filtered video values hide the Group by section", async () => {
    await evaluate(() => { mg.catalog.items = mg.catalog.items.map(item => ({ ...item, videoCodec: null, videoDuration: null, videoFrameRate: null })); });
    await settle();
    assert(await evaluate(() => mg.session.groupDims.length === 0 && mg.session.activeGroupBy.length === 0), "Unknown values keep a grouping dimension");
    assert(await evaluate(() => ![...document.querySelectorAll('[role="dialog"][aria-label="View"] section')].some(section => section.textContent.includes("Group by"))), "The empty section stays visible");
    await evaluate(() => { mg.catalog.items[1].videoCodec = "hevc"; mg.catalog.items[0].videoCodec = "h264"; mg.session.groupBy = ["codec"]; mg.session.nameFilter = "0"; });
    await settle();
    assert(await evaluate(() => mg.session.activeGroupBy.length === 0), "Filtering to one value leaves a group header");
  });
  await check("Runtime checks report affected desktop tools and cancel stale results", async () => {
    await evaluate(() => {
      mg.originalProbe = mg.api.runtimeTools;
      mg.originalCanPlayType = HTMLMediaElement.prototype.canPlayType;
      mg.originalMseSupport = typeof MediaSource === "undefined" ? null : MediaSource.isTypeSupported;
      HTMLMediaElement.prototype.canPlayType = () => "";
      if (typeof MediaSource !== "undefined") MediaSource.isTypeSupported = () => false;
      mg.probeStatus = { platform: "windows", ffmpegPresent: false, ffmpegMajor: null, ffprobePresent: false, heifAvailable: false, hasHeif: true, hasVideos: true, hasHevc: true };
      mg.api.runtimeTools = async () => mg.probeStatus;
      mg.session.viewPanelOpen = false;
      mg.view.shortcutsOpen = true;
    });
    try {
      await settle();
      await evaluate(() => mg.runtimeTools.check());
      await settle();
      assert(await evaluate(() => {
        const warning = document.querySelector('[aria-label="Missing media tools"]');
        const shortcuts = document.querySelector('[role="dialog"][aria-label="Keyboard shortcuts"]');
        return Number(getComputedStyle(warning.parentElement).zIndex) > Number(getComputedStyle(shortcuts.parentElement).zIndex) && warning.contains(document.activeElement);
      }), "The warning is hidden behind shortcut help or has no modal focus");
      await evaluate(() => { mg.view.shortcutsOpen = false; });
      assert(await evaluate(() => mg.runtimeTools.pending.map(issue => issue.id).sort().join() === "ffmpeg,ffprobe,heif,hevc"), "Missing desktop media tools are not reported");
      assert(await evaluate(() => document.querySelector('[aria-label="Missing media tools"]')?.textContent.includes("does not report HEVC playback support")), "The HEVC warning overstates package detection");
      const shot = await send("Page.captureScreenshot", { format: "png" });
      await writeFile(join(output, "missing-media-tools.png"), Buffer.from(shot.data, "base64"));
      await evaluate(async () => { mg.probeStatus = { ...mg.probeStatus, platform: "android" }; await mg.runtimeTools.check(); });
      assert(await evaluate(() => mg.runtimeTools.pending.length === 0), "Android is asked to install desktop tools");
      await evaluate(async () => { mg.probeStatus = { ...mg.probeStatus, platform: "windows", hasHeif: false, hasVideos: false, hasHevc: false }; await mg.runtimeTools.check(); });
      assert(await evaluate(() => mg.runtimeTools.pending.length === 0), "A RAW/JPEG project shows unrelated warnings");
      await evaluate(async () => {
        mg.api.runtimeTools = () => new Promise(resolve => mg.finishProbe = resolve);
        const task = mg.runtimeTools.check();
        mg.runtimeTools.cancel();
        mg.finishProbe({ ...mg.probeStatus, hasHeif: true });
        await task;
      });
      assert(await evaluate(() => mg.runtimeTools.pending.length === 0), "A cancelled project probe shows a stale warning");
    } finally {
      await evaluate(() => {
        mg.api.runtimeTools = mg.originalProbe;
        mg.view.shortcutsOpen = false;
        HTMLMediaElement.prototype.canPlayType = mg.originalCanPlayType;
        if (typeof MediaSource !== "undefined") MediaSource.isTypeSupported = mg.originalMseSupport;
        mg.runtimeTools.cancel();
      });
    }
  });
  await check("Missing tools can be acknowledged and suppressed per tool", async () => {
    await evaluate(() => { mg.session.viewPanelOpen = false; mg.runtimeTools.offer([{ id: "ffmpeg", name: "FFmpeg", detail: "Video thumbnails need FFmpeg on PATH.", url: "https://ffmpeg.org/download.html" }]); });
    await settle();
    assert(await evaluate(() => !!document.querySelector('[aria-label="Missing media tools"]')), "The tool warning is missing");
    await evaluate(() => document.querySelector('[aria-label="Missing media tools"] button[data-action="ok"]').click());
    await settle();
    assert(await evaluate(() => !document.querySelector('[aria-label="Missing media tools"]')), "OK does not close the warning");
    await evaluate(() => { mg.runtimeTools.offer([{ id: "ffprobe", name: "FFprobe", detail: "Video properties need FFprobe.", url: "https://ffmpeg.org/download.html" }]); });
    await settle();
    await evaluate(() => document.querySelector('[aria-label="Missing media tools"] button[data-action="suppress"]').click());
    assert(await evaluate(() => JSON.parse(localStorage.getItem("cullant.hiddenRuntimeTools")).includes("ffprobe")), "Suppression is not saved");
  });
} finally {
  if (original) await evaluate(async saved => {
    mg.settings.setRememberSession(false);
    if (saved.root && saved.root !== mg.catalog.project?.rootPath) await mg.catalog.open(saved.root);
    if (!saved.root && mg.catalog.project) await mg.catalog.close();
    if (saved.root) await mg.catalog.setMedia(saved.media);
    mg.catalog.items = saved.items; mg.session.clearFilters(); mg.session.viewPanelOpen = false;
    for (const media of ["photos", "videos"]) {
      mg.catalog.media = media;
      mg.session.groupBy = saved.groups.groupByMedia?.[media] ?? (media === saved.media ? saved.groups.groupBy : []);
    }
    mg.catalog.media = saved.media; mg.runtimeTools.dismiss(false);
    mg.folders.restoreScope(saved.scope);
    localStorage.clear(); for (const [key, value] of Object.entries(saved.storage)) localStorage.setItem(key, value);
  }, original).catch(() => {});
  if (originalRemember !== undefined) await evaluate(saved => mg.settings.setRememberSession(saved), originalRemember).catch(() => {});
  await writeFile(join(output, "results.json"), JSON.stringify({ results, observations }, null, 2));
  socket.close();
}
if (results.some(result => result.status === "FAIL")) process.exitCode = 1;
