// E2E measurement of a new root project. Source files are never changed.
// Failure modes: wrong root, existing project deletion, missing output, hidden
// decoder errors, navigation stalls, and cache hits mistaken for generation.
import { readFile, writeFile, mkdir, unlink, stat } from "node:fs/promises";
import { resolve, join, relative, isAbsolute } from "node:path";
import { DatabaseSync } from "node:sqlite";
import assert from "node:assert/strict";

const args = Object.fromEntries(process.argv.slice(2).map(a => a.replace(/^--/, "").split("=")));
const root = resolve(args.root ?? "D:/");
const output = resolve(args.output ?? "artifacts/performance/2026-10-01-d-drive");
const run = args.run ?? "first-2560";
const edge = Number(args.edge ?? 2560);
assert([1600, 2560, 3840].includes(edge));
assert(/^[\w-]+$/.test(run));
await mkdir(output, { recursive: true });
const ownership = join(output, "owned-project.json");
const dbPath = join(root, ".cullant", "cullant.db");
const pause = ms => new Promise(r => setTimeout(r, ms));
const exists = path => stat(path).then(() => true, () => false);

if (args.prepare === "first") {
  assert(!await exists(join(root, ".cullant")), "The first run requires a new root project");
  assert(!await exists(ownership), "Use a new artifact directory");
  await writeFile(ownership, JSON.stringify({ root, created: new Date().toISOString() }));
  process.exit(0);
}
if (args.prepare === "reset") {
  assert.equal(JSON.parse(await readFile(ownership, "utf8")).root, root);
  const db = new DatabaseSync(dbPath);
  try {
    const state = db.prepare("SELECT root_path FROM project WHERE id=1").get();
    assert.equal(resolve(state.root_path), root);
    const cache = resolve(root, ".cullant", "thumbs");
    for (const row of db.prepare("SELECT cache_path FROM thumbnails WHERE kind IN (0,1)").all()) {
      if (!row.cache_path) continue;
      const target = resolve(cache, row.cache_path), rel = relative(cache, target);
      assert(rel && !rel.startsWith("..") && !isAbsolute(rel), "Cache path escaped root");
      await unlink(target).catch(e => { if (e.code !== "ENOENT") throw e; });
    }
    db.exec("DELETE FROM thumbnails WHERE kind IN (0,1)");
  } finally { db.close(); }
  process.exit(0);
}

let target;
for (let i = 0; i < 120 && !target; i++) {
  try { target = (await (await fetch("http://127.0.0.1:9223/json/list")).json()).find(t => t.type === "page"); } catch {}
  if (!target) await pause(500);
}
assert(target, "Release app CDP endpoint missing");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((ok, fail) => { socket.onopen = ok; socket.onerror = fail; });
let sequence = 0;
const pending = new Map();
socket.onmessage = ({ data }) => {
  const row = JSON.parse(data), task = pending.get(row.id);
  if (!task) return;
  pending.delete(row.id); clearTimeout(task.timer);
  row.error ? task.fail(new Error(row.error.message)) : task.ok(row.result);
};
function send(method, params = {}) {
  return new Promise((ok, fail) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); fail(new Error(`CDP timeout: ${method}`)); }, 120000);
    pending.set(id, { ok, fail, timer }); socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const row = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (row.exceptionDetails) throw new Error(row.exceptionDetails.exception?.description ?? row.exceptionDetails.exception?.value ?? JSON.stringify(row.exceptionDetails));
  return row.result.value;
}
async function screenshot(name) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  await writeFile(join(output, `${run}-${name}.png`), Buffer.from(shot.data, "base64"));
}
const preferenceScript = `localStorage.setItem('cullant.generateVideoThumbs','false');localStorage.setItem('cullant.previewQuality','${edge}');`;
await send("Page.addScriptToEvaluateOnNewDocument", { source: preferenceScript });
await evaluate(preferenceScript);
await send("Page.reload");
await pause(1800);
await evaluate(`window.profileEvents=[]; window.profileInvoke=window.__TAURI_INTERNALS__.invoke.bind(window.__TAURI_INTERNALS__);`);
await evaluate("window.profileInvoke('plugin:window|show',{label:'main'}).catch(()=>{})");
await evaluate(`(async()=>{for(const event of ['scan:done','metadata:done','thumbs:progress','previews:progress','scan:error']){
  const handler=window.__TAURI_INTERNALS__.transformCallback(message=>window.profileEvents.push({event,at:Date.now(),payload:message.payload}));
  await window.profileInvoke('plugin:event|listen',{event,target:{kind:'Any'},handler});}})()`);
const start = Date.now();
await evaluate(`(()=>{const card=[...document.querySelectorAll('button.card')].find(b=>b.title===${JSON.stringify(root)});const button=card??[...document.querySelectorAll('button')].find(b=>b.textContent.includes('Open new project'));if(!button)throw Error('Open project button missing');button.click();})()`);
const navigation = [];
let firstGridMs = null, complete = false, firstScreen = false;
for (let i = 0; i < 1800; i++) {
  const status = await evaluate(`({events:window.profileEvents,images:[...document.querySelectorAll('.cell img')].filter(i=>i.complete&&i.naturalWidth>0).length,cells:document.querySelectorAll('.cell').length})`);
  if (status.events.some(e => e.event === "scan:error")) throw new Error(JSON.stringify(status.events));
  if (firstGridMs === null && status.images >= 12) firstGridMs = Date.now() - start;
  if (!firstScreen && status.images >= 12) { await screenshot("preparing"); firstScreen = true; }
  if (!args.quiet && navigation.length < 12 && firstGridMs !== null) {
    const t = Date.now();
    const loaded = await evaluate(`(async()=>{
      const cells=[...document.querySelectorAll('.cell')];const cell=cells[${navigation.length}%cells.length];
      if(${navigation.length}===0 && cell){const box=cell.getBoundingClientRect();cell.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,clientX:box.x+box.width/2,clientY:box.y+box.height/2}));}
      window.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowRight',code:'ArrowRight',bubbles:true}));
      await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
      const target=document.querySelector('.viewer .filename')?.textContent;
      window.profileItems??=await window.profileInvoke('query_items',{media:'photos',sort:'capture',desc:false});
      const id=window.profileItems.find(i=>i.relPath===target)?.id;
      const began=performance.now(),deadline=began+5000;
      while(performance.now()<deadline){const img=document.querySelector('.viewer img.fit');if(id&&img?.currentSrc.includes('/preview/'+id+'?')&&img.complete&&img.naturalWidth>0){await new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));return {targetId:id,previewReadyMs:performance.now()-began,timedOut:false};}await new Promise(r=>setTimeout(r,20));}return {targetId:id,previewReadyMs:performance.now()-began,timedOut:true};
    })()`);
    navigation.push({ atMs: t - start, roundTripMs: Date.now() - t, targetPreview: loaded });
  }
  const lastPreview = status.events.filter(e => e.event === "previews:progress").at(-1)?.payload;
  if (lastPreview && lastPreview.done >= lastPreview.total) {
    complete = true; if(!args.measureNavigation || navigation.length >= 12) break;
  }
  await pause(500);
}
assert(complete, "Preparation timed out");
if (args.measureNavigation) {
  assert.equal(navigation.length, 12, "Navigation sample incomplete");
  assert(navigation.every(row => row.targetPreview.targetId && !row.targetPreview.timedOut), "Requested preview did not load");
}
const elapsedMs = Date.now() - start;
await pause(1500);
await screenshot("complete");
const events = await evaluate("window.profileEvents");
const db = new DatabaseSync(dbPath, { readOnly: true });
let artifacts, counts;
try {
  artifacts = db.prepare("SELECT t.file_id,t.kind,t.cache_path,t.failed,t.width,t.height,t.long_edge FROM thumbnails t JOIN files f ON f.id=t.file_id WHERE f.status=0 AND f.kind IN(0,1) AND t.kind IN(0,1)").all();
  counts = db.prepare("SELECT kind,count(*) AS count FROM files WHERE status=0 GROUP BY kind").all();
} finally { db.close(); }
assert(artifacts.some(r => r.kind === 1), "No previews");
assert(artifacts.every(r => !r.failed), "A photo failed to render");
for (const row of artifacts) {
  assert((await stat(join(root, ".cullant", "thumbs", row.cache_path))).size > 0);
  assert(Math.max(row.width, row.height) <= (row.kind === 0 ? 384 : edge));
}
const result = { run, edge, start, elapsedMs, firstGridMs, navigation, events, counts, artifacts, status: "PASS" };
await writeFile(join(output, `${run}-result.json`), JSON.stringify(result, null, 2));
await evaluate("window.profileInvoke('close_project')");
socket.close();
console.log(JSON.stringify({ run, elapsedMs, firstGridMs, artifacts: artifacts.length, status: "PASS" }));
