import { mkdir, readFile, rename, unlink, utimes, writeFile } from "node:fs/promises";
import { resolve, relative, isAbsolute, join } from "node:path";
import { deflateSync } from "node:zlib";
import { DatabaseSync } from "node:sqlite";

const fixture = resolve(".playwright-mcp/preview-cache-project");
const output = resolve(process.argv.find(arg => arg.startsWith("--output="))?.slice(9) ?? ".playwright-mcp/preview-cache");
const count = 32;
const baseline = process.argv.includes("--baseline");
const terminalOnly = process.argv.includes("--terminal-only");
const membersOnly = process.argv.includes("--members-only");
const legacyOnly = process.argv.includes("--legacy-only");
const extraOnly = process.argv.includes("--extra-only") || terminalOnly || membersOnly || legacyOnly;

function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return (crc ^ 0xffffffff) >>> 0;
}

function pngChunk(type, bytes) {
  const body = Buffer.concat([Buffer.from(type), bytes]);
  const length = Buffer.alloc(4); length.writeUInt32BE(bytes.length);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

function fixtureImage() {
  const width = 3072, height = 2048, stride = 1 + width * 3;
  const pixels = Buffer.alloc(height * stride);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const offset = y * stride + 1 + x * 3;
      pixels[offset] = ((x >> 3) + (y >> 3)) % 256;
      pixels[offset + 1] = ((x >> 4) ^ (y >> 4)) % 256;
      pixels[offset + 2] = ((x >> 4) * 3 + (y >> 4) * 5) % 256;
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), pngChunk("IHDR", header), pngChunk("IDAT", deflateSync(pixels)), pngChunk("IEND", Buffer.alloc(0))]);
}

if (process.argv.includes("--prepare")) {
  await mkdir(fixture, { recursive: true });
  const bytes = fixtureImage();
  for (let index = 0; index < count; index++) {
    const path = join(fixture, `photo-${String(index + 1).padStart(3, "0")}.png`);
    await writeFile(path, bytes);
    await utimes(path, 1700000000 + index * 120, 1700000000 + index * 120);
  }
  console.log(fixture);
  process.exit(0);
}

// Failure modes: lost cache with surviving rows; empty and truncated JPEGs;
// corrupt JPEG pixels; late completion events; stale ready ids; warm loupe blur.
const port = process.env.CULLANT_CDP_PORT ?? "9222";
const targets = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const target = targets.find(page => page.type === "page" && page.url.includes("localhost:1420"));
if (!target) throw new Error("Start npm run tauri:debug with CULLANT_OPEN_PROJECT set to the prepared preview-cache-project.");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let sequence = 0;
const pending = new Map();
const runtimeErrors=[];
socket.onmessage = ({ data }) => {
  const reply = JSON.parse(data), task = pending.get(reply.id);
  if (reply.method==='Runtime.exceptionThrown' || reply.method==='Log.entryAdded' && reply.params.entry.level==='error') runtimeErrors.push(reply.params);
  if (!task) return;
  pending.delete(reply.id);
  if (reply.error) task.reject(new Error(reply.error.message));
  else task.resolve(reply.result);
};
socket.onclose=()=>{for(const task of pending.values()) task.reject(new Error('App CDP connection closed'));pending.clear();};
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const reply = await send("Runtime.evaluate", { expression: `(async () => {const json=JSON.stringify(await eval(${JSON.stringify(expression)}));return json===undefined?null:JSON.parse(json);})()`, awaitPromise: true, returnByValue: true });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result.value;
}
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function waitFor(expression, timeout = 45000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (await evaluate(expression)) return;
    await pause(50);
  }
  throw new Error(`Timed out: ${expression}`);
}
function assert(condition, message) { if (!condition) throw new Error(message); }
function cachePath(path) {
  const root = join(fixture, ".cullant", "thumbs"), target = resolve(root, path);
  const rel = relative(root, target);
  assert(rel && !rel.startsWith("..") && !isAbsolute(rel), "Cache path leaves the disposable project");
  return target;
}
function rows() {
  const db = new DatabaseSync(join(fixture, ".cullant", "cullant.db"), { readOnly: true });
  try { return db.prepare("SELECT t.file_id AS id, t.kind, t.cache_path AS path, t.failed, t.generated_at AS generated, t.long_edge AS edge FROM thumbnails t JOIN files f ON f.id=t.file_id WHERE f.status=0 ORDER BY t.file_id,t.kind").all(); }
  finally { db.close(); }
}
async function screenshot(name) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  await writeFile(join(output, `${name}.png`), Buffer.from(shot.data, "base64"));
}
async function state() {
  return evaluate("({items:pc.catalog.items.map(item => ({id:item.id,thumbReady:item.thumbReady,thumbFailed:item.thumbFailed,previewFailed:item.previewFailed})),ready:[...pc.catalog.previewReady],ingesting:pc.catalog.ingesting,progress:pc.catalog.previewProgress,spinners:document.querySelectorAll('.cell .preview-spin').length,error:pc.catalog.error})");
}
const results = [], observations = [];
async function check(name, action) {
  try { await action(); results.push({ name, status: "PASS" }); }
  catch (error) { results.push({ name, status: "FAIL", error: error.message }); await screenshot(`failure-${results.length}`).catch(() => {}); }
  console.log(`${results.at(-1).status}: ${name}${results.at(-1).error ? `: ${results.at(-1).error}` : ""}`);
}
async function warm() {
  if (await evaluate("pc.expectPreviewPass")) await waitFor("pc.previewEvents.some(event=>event.done>=event.total)");
  await waitFor(`pc.catalog.items.length === ${count} && !pc.catalog.ingesting && !pc.session.restoring`);
  await evaluate("pc.expectPreviewPass=false");
  await evaluate("pc.catalog.refreshPreviewReady()");
  await waitFor(`pc.catalog.previewReady.size === ${count}`);
  const previews = rows().filter(row => row.kind === 1 && !row.failed);
  assert(previews.length === count, "Import did not store every preview row");
  for (const row of previews) assert((await readFile(cachePath(row.path))).length > 100, `Preview ${row.id} has no cached bytes`);
  return previews;
}
async function reopen() {
  await evaluate(`pc.previewEvents=[];pc.expectPreviewPass=true;pc.catalog.open(${JSON.stringify(fixture)})`);
  await waitFor(`pc.catalog.items.length === ${count} && !pc.catalog.preloading && !pc.session.restoring`);
  await waitFor("pc.previewEvents.some(event=>event.done>=event.total)");
}
async function rebuild() {
  await evaluate("pc.api.discardPreviews();");
  await evaluate("pc.previewEvents=[];pc.expectPreviewPass=true;pc.api.rescanProject()");
  return warm();
}

await mkdir(output, { recursive: true });
let original;
try {
  await send('Runtime.enable'); await send('Log.enable');
  await send('Page.reload');
  for(let attempt=0;attempt<100;attempt++) {
    if(await evaluate("performance.getEntriesByType('resource').some(entry=>new URL(entry.name).pathname==='/src/lib/stores/catalog.svelte.ts')").catch(()=>false)) break;
    await pause(100);
  }
  await pause(250);
  await evaluate(`(async () => {
    const loaded = path => import(performance.getEntriesByType('resource').find(entry => new URL(entry.name).pathname === path)?.name ?? path);
    const {catalog} = await loaded('/src/lib/stores/catalog.svelte.ts');
    const {session} = await loaded('/src/lib/stores/session.svelte.ts');
    const {settings} = await loaded('/src/lib/stores/settings.svelte.ts');
    const {view} = await loaded('/src/lib/stores/view.svelte.ts');
    const {api,previewUrl} = await loaded('/src/lib/api.ts');
    const path = catalog.project?.rootPath.replace(String.fromCharCode(92,92,63,92),'').replaceAll('\\\\','/').toLowerCase();
    if (path && path !== ${JSON.stringify(fixture.replaceAll("\\", "/").toLowerCase())}) throw new Error('Use the disposable preview-cache-project fixture.');
    window.pc?.stopProgress?.();
    window.pc = {catalog,session,settings,view,api,previewUrl,previewEvents:[],expectPreviewPass:false};
    const {listen}=await import('/node_modules/.vite/deps/@tauri-apps_api_event.js');
    pc.stopProgress=await listen('previews:progress',event=>{if(event.payload.projectRoot.toLowerCase().includes('preview-cache-project')) pc.previewEvents.push(event.payload);});
    if (!path) await catalog.open(${JSON.stringify(fixture)});
  })()`);
  original = await evaluate("({remember:pc.settings.rememberSession,progressive:pc.settings.progressiveLoupe,collapse:pc.settings.collapseBursts,quality:pc.settings.previewQuality,mirror:pc.session.mirrorMode})");
  original.quality=2560;
  await evaluate("pc.settings.setPreviewQuality(2560);pc.api.setPreviewQuality(2560)");
  await evaluate("pc.settings.setRememberSession(false); pc.settings.setProgressiveLoupe(true); pc.settings.collapseBursts=false; pc.session.clearFilters(); pc.view.mode='grid'");
  const existing=rows().filter(row=>row.kind===1 && !row.failed);
  if (rows().some(row=>row.kind===1 && row.failed) || (await Promise.all(existing.map(row=>readFile(cachePath(row.path)).catch(()=>null)))).some(bytes=>!bytes?.length)) await rebuild();
  let previews = await warm();
  await screenshot("import-complete");
  if (legacyOnly || !baseline && !extraOnly) {
    await check("A valid source with an old failure row recovers once when reopening", async () => {
      const row=previews[0];
      await evaluate("pc.catalog.close()");
      await unlink(cachePath(row.path));
      const db=new DatabaseSync(join(fixture,'.cullant','cullant.db'));
      try {
        db.prepare('UPDATE thumbnails SET failed=1 WHERE file_id=? AND kind=1').run(row.id);
        db.prepare("DELETE FROM settings WHERE key='cacheFailureRecovery'").run();
      } finally { db.close(); }
      try {
        await reopen();
        const repaired=rows().find(value=>value.id===row.id && value.kind===1);
        observations.push({case:'legacy-failure-recovery',row:repaired,state:await state()});
        assert(repaired && !repaired.failed,'Historical failure row blocks a valid source preview');
        assert((await readFile(cachePath(repaired.path)).catch(()=>null))?.length>1000,'Historical preview failure was not regenerated');
      } finally { await rebuild(); }
    });
    previews=await warm();
  }
  if (!extraOnly) {
  await check("A warm reopen keeps all cached previews without regeneration", async () => {
    const before = previews.map(row => [row.id, row.path, row.generated]);
    await reopen(); previews = await warm();
    assert(JSON.stringify(before) === JSON.stringify(previews.map(row => [row.id,row.path,row.generated])), "Warm reopen regenerated valid previews");
  });
  await check("A fully cached photo opens the loupe without the soft thumbnail", async () => {
    const observed = await evaluate(`(async () => {
      const sources=[];
      const capture=()=>{for(const img of document.querySelectorAll('.viewer .stage img')) sources.push(img.getAttribute('src'));};
      const observer=new MutationObserver(capture); observer.observe(document.body,{subtree:true,childList:true,attributes:true,attributeFilter:['src']});
      pc.session.focusedIndex=0; pc.view.mode='viewer';
      await new Promise(resolve=>setTimeout(resolve,250)); capture(); observer.disconnect();
      return {sources,images:[...document.querySelectorAll('.viewer img')].map(img=>({src:img.src,width:img.naturalWidth,complete:img.complete})),focus:pc.session.focusedIndex,mode:pc.view.mode,filtered:pc.session.filtered.length,body:document.body.textContent.slice(-2000)};
    })()`);
    observations.push({case:"warm-loupe",...observed});
    assert(observed.sources.length > 0, "Loupe image selector did not match");
    assert(!observed.sources.some(src => src?.includes('/thumb/')), "Ready photo first shows the soft grid thumbnail");
    await screenshot("warm-loupe");
    await evaluate("pc.view.mode='grid'");
  });
  await check("Reopen repairs lost preview files and shows thumbnail spinners", async () => {
    await evaluate("pc.view.mode='grid'; pc.catalog.close()");
    for (const row of previews) await unlink(cachePath(row.path));
    await evaluate(`pc.previewEvents=[];pc.expectPreviewPass=true;pc.trace=[]; pc.traceObserver=new MutationObserver(()=>{const snapshot={at:performance.now(),ready:pc.catalog.previewReady.size,spinners:document.querySelectorAll('.cell .preview-spin').length,progress:{...pc.catalog.previewProgress}}; if(snapshot.spinners || snapshot.progress.total) pc.trace.push(snapshot);}); pc.traceObserver.observe(document.body,{subtree:true,childList:true,attributes:true}); pc.catalog.open(${JSON.stringify(fixture)})`);
    await waitFor("pc.previewEvents.some(event=>event.done>=event.total)");
    await waitFor(`pc.catalog.items.length === ${count} && !pc.catalog.ingesting && !pc.session.restoring`);
    await evaluate("pc.catalog.refreshPreviewReady()");
    const trace = await evaluate("pc.traceObserver.disconnect(); pc.trace");
    observations.push({case:"lost-preview-reopen",trace,state:await state()});
    const missing=[];
    for (const row of rows().filter(row=>row.kind===1 && !row.failed)) {
      if (!(await readFile(cachePath(row.path)).catch(()=>null))?.length) missing.push(row.id);
    }
    assert(missing.length===0, `Reopen claims readiness but ${missing.length} preview files are absent: ${missing.join(',')}`);
    assert((await evaluate("pc.api.previewReadyIds()")).length===count, "Repaired preview readiness is incomplete");
    assert(trace.some(snapshot=>snapshot.spinners>0), "Missing previews show no thumbnail spinners during repair");
    previews=await warm(); await screenshot("lost-previews-repaired");
  });
  }
  if (!baseline && !extraOnly) {
    if (results.some(result=>result.status==='FAIL')) previews=await rebuild();
    await check("Reopen repairs empty, truncated, and damaged JPEG cache files", async () => {
      await evaluate("pc.catalog.close()");
      const selected=previews.slice(0,3), bytes=await readFile(cachePath(selected[2].path));
      await writeFile(cachePath(selected[0].path),Buffer.alloc(0));
      await writeFile(cachePath(selected[1].path),Buffer.from([0xff,0xd8,0xff,0xd9]));
      await writeFile(cachePath(selected[2].path),Buffer.concat([bytes.subarray(0,40),Buffer.alloc(200,0x42),Buffer.from([0xff,0xd9])]));
      await reopen(); await warm();
      const repaired=rows().filter(row=>row.kind===1 && selected.some(item=>item.id===row.id));
      for(const row of repaired) assert((await readFile(cachePath(row.path))).length>1000,`Corrupt preview ${row.id} survives reopening`);
      const decoded=await evaluate(`Promise.all(pc.catalog.items.slice(0,3).map(item=>new Promise(resolve=>{const img=new Image();img.onload=()=>resolve({id:item.id,width:img.naturalWidth});img.onerror=()=>resolve({id:item.id,width:0});img.src=pc.previewUrl(item)+'&test=corruption-'+Date.now();})))`);
      observations.push({case:"corrupt-cache",decoded});
      assert(decoded.every(item=>item.width>384),"Repaired preview cannot be decoded at loupe resolution");
    });
    if (results.at(-1).status==='FAIL') previews=await rebuild();
    await check("A ready-id refresh removes a preview that disappears mid-session", async () => {
      const row=rows().find(row=>row.kind===1 && !row.failed);
      await unlink(cachePath(row.path));
      await evaluate("pc.catalog.refreshPreviewReady()");
      const ready=await evaluate(`pc.catalog.previewReady.has(${row.id})`);
      observations.push({case:"mid-session-loss",id:row.id,state:await state()});
      assert(!ready,"Ready-id refresh retains a deleted preview");
      await screenshot("mid-session-loss");
      await evaluate("pc.previewEvents=[];pc.expectPreviewPass=true;pc.api.rescanProject()"); await warm();
    });
    if (results.at(-1).status==='FAIL') previews=await rebuild();
    await check("A changed preview quality is checked when the project reopens", async () => {
      const quality=original.quality===1600?2560:1600;
      await evaluate(`pc.settings.setPreviewQuality(${quality});pc.api.setPreviewQuality(${quality})`);
      await reopen(); await warm();
      const current=rows().filter(row=>row.kind===1 && !row.failed);
      assert(current.every(row=>row.edge===quality),"Ready previews still use the previous quality after reopening");
      observations.push({case:"quality-reopen",quality,edges:current.map(row=>row.edge)});
    });
    await evaluate(`pc.settings.setPreviewQuality(${original.quality});pc.api.setPreviewQuality(${original.quality})`);
    if (results.at(-1).status==='FAIL') previews=await rebuild();
    await check("A failed preview never becomes ready through a progress event", async () => {
      const path=join(fixture,'photo-032.png'), bytes=await readFile(path);
      await evaluate("pc.catalog.close()");
      await writeFile(path,Buffer.from('Corrupt source for the preview cache regression'));
      await utimes(path,1700010000,1700010000);
      try {
        await reopen(); await waitFor("pc.catalog.items.some(item=>item.relPath==='photo-032.png' && (item.previewFailed || item.thumbFailed))");
        await evaluate("Promise.all([pc.catalog.refresh(),pc.catalog.refreshPreviewReady()])");
        const item=await evaluate("JSON.parse(JSON.stringify(pc.catalog.items.find(item=>item.relPath==='photo-032.png')))");
        observations.push({case:"failed-generation",item,state:await state()});
        assert(item.previewFailed || item.thumbFailed,"Decode failure has no terminal failure state");
        assert(!await evaluate(`pc.catalog.previewReady.has(${item.id})`),"Failed preview progress reports the file as ready");
        const failure=rows().find(row=>row.id===item.id && row.kind===1);
        assert(failure?.failed,"Corrupt source has no preview failure row");
        await pause(1100); await reopen();
        const retained=rows().find(row=>row.id===item.id && row.kind===1);
        assert(retained?.failed && retained.generated===failure.generated,"A new decode failure is retried on every reopen");
      } finally {
        await evaluate("pc.catalog.close()");
        await writeFile(path,bytes); await utimes(path,1700000000+31*120,1700000000+31*120);
        await reopen(); await warm();
      }
    });
    await check("Adopting a project after its completion events seeds valid readiness", async () => {
      await evaluate("pc.catalog.close()");
      await evaluate(`pc.api.openProject(${JSON.stringify(fixture)})`);
      await pause(1000);
      await evaluate("pc.catalog.adoptCurrent()"); await warm();
      assert(await evaluate(`pc.catalog.previewReady.size===${count} && document.querySelectorAll('.cell .preview-spin').length===0`),"Missed completion events leave cached preview spinners");
    });
  }
  if (!baseline && !membersOnly && !legacyOnly) {
    await check("A failed preview pass ends its spinner and offers a retry warning", async () => {
      const id=previews[0].id, ready=await evaluate("pc.api.previewReadyIds()");
      await waitFor(`pc.catalog.thumbLoaded.has(${id})`);
      await evaluate(`pc.originalReady=pc.api.previewReadyIds;pc.api.previewReadyIds=()=>Promise.resolve(${JSON.stringify(ready.filter(value=>value!==id))});pc.catalog.previewReady.delete(${id})`);
      try {
        await evaluate(`(async()=>{const {emit}=await import('/node_modules/.vite/deps/@tauri-apps_api_event.js');await emit('previews:progress',{projectRoot:pc.catalog.project.rootPath,done:1,total:1,ids:[]});})()`);
        await evaluate("Promise.all([pc.catalog.refresh(),pc.catalog.refreshPreviewReady()])");
        await pause(100);
        observations.push({case:"failed-pass-warning",id,state:await state()});
        assert(await evaluate("document.querySelectorAll('.cell .preview-spin').length===0"),"Failed preview pass leaves a thumbnail spinner running");
        assert(await evaluate("!!document.querySelector('.cell [title=\"Preview unavailable. Rescan to retry.\"]')"),"Failed preview has no visible retry warning");
        await screenshot("failed-pass-warning");
      } finally {
        await evaluate("pc.api.previewReadyIds=pc.originalReady");
        await reopen(); await warm();
      }
    });
    if (!terminalOnly) {
    await check("A late ready-id response keeps successful progress received while it waits", async () => {
      const id=previews.at(-1).id;
      const response=await evaluate(`pc.api.previewReadyIds()`);
      await evaluate(`pc.originalReady=pc.api.previewReadyIds;pc.api.previewReadyIds=()=>new Promise(resolve=>pc.resolveReady=resolve);pc.catalog.previewReady.delete(${id});pc.refreshWaiting=pc.catalog.refreshPreviewReady();void 0`);
      try {
        await evaluate(`(async()=>{const {emit}=await import('/node_modules/.vite/deps/@tauri-apps_api_event.js');await emit('previews:progress',{projectRoot:pc.catalog.project.rootPath,done:1,total:${count},ids:[${id}]});})()`);
        await waitFor(`pc.catalog.previewReady.has(${id})`);
        await evaluate(`pc.resolveReady(${JSON.stringify(response.filter(value=>value!==id))});pc.refreshWaiting`);
        assert(await evaluate(`pc.catalog.previewReady.has(${id})`),"A delayed readiness response erases a valid progress id");
      } finally {
        await evaluate("pc.api.previewReadyIds=pc.originalReady;pc.catalog.previewProgress={done:0,total:0};pc.catalog.refreshPreviewReady()");
      }
    });
    await check("A temporary source read failure does not block later preview repair", async () => {
      previews=await warm();
      const row=previews.at(-1), source=join(fixture,'photo-032.png'), absent=source+'.absent';
      await unlink(cachePath(row.path));
      await rename(source,absent);
      try {
        const loaded=await evaluate(`new Promise(resolve=>{const item=pc.catalog.items.find(item=>item.id===${row.id}),img=new Image();img.onload=()=>resolve(true);img.onerror=()=>resolve(false);img.src=pc.previewUrl(item)+'&test=source-absent-'+Date.now();})`);
        assert(!loaded,"Absent source still returned a preview; this case did not force a source read");
      } finally { await rename(absent,source); }
      await evaluate("pc.previewEvents=[];pc.api.rescanProject()");
      await waitFor("pc.previewEvents.some(event=>event.done>=event.total)");
      await waitFor("!pc.catalog.ingesting");
      await evaluate("Promise.all([pc.catalog.refresh(),pc.catalog.refreshPreviewReady()])");
      const current=rows().find(value=>value.kind===1 && value.id===row.id);
      observations.push({case:"transient-source-failure",row:current,state:await state()});
      assert(current && !current.failed,"A temporary source read failure leaves a failure tombstone");
      assert((await readFile(cachePath(current.path)).catch(()=>null))?.length>1000,"Restored source does not regenerate its missing preview");
    });
    }
  }
  if (membersOnly || !baseline && !extraOnly) {
    await check("Separate mode prepares a visible nonprimary member without opening the loupe", async () => {
      const raw=join(fixture,'photo-001.cr3');
      await evaluate("pc.catalog.close()");
      const db=new DatabaseSync(join(fixture,'.cullant','cullant.db'));
      try { db.exec('PRAGMA foreign_keys=ON');db.prepare("DELETE FROM files WHERE rel_path='photo-001.cr3' AND status<>0").run(); }
      finally { db.close(); }
      await unlink(cachePath(previews.find(row=>row.id===1).path));
      await writeFile(raw,Buffer.from('raw')); await utimes(raw,1700000000,1700000000);
      try {
        await evaluate(`pc.previewEvents=[];pc.catalog.open(${JSON.stringify(fixture)})`);
        await waitFor(`pc.catalog.items.length===${count+1} && pc.previewEvents.some(event=>event.done>=event.total)`);
        await evaluate("pc.session.setMirrorMode(false);pc.session.clearFilters();pc.view.mode='grid'");
        const member=await evaluate("JSON.parse(JSON.stringify(pc.catalog.items.find(item=>item.relPath==='photo-001.png')))");
        assert(Number.isInteger(member.id),"Member does not have a numeric file id");
        assert(!member.isPrimary,"PNG did not become a nonprimary member");
        await waitFor(`pc.catalog.thumbLoaded.has(${member.id})`,10000);
        await waitFor(`pc.catalog.previewReady.has(${member.id})`,10000);
        observations.push({case:"separate-member",member,state:await state()});
        assert(await evaluate("pc.view.mode==='grid'"),"Member was prepared by entering the loupe");
        await screenshot("separate-member");
      } finally {
        observations.push({case:"separate-member-final",state:await state()});
        await evaluate("pc.catalog.close()"); await unlink(raw);
        await reopen(); await warm();
        await evaluate(`pc.session.setMirrorMode(${original.mirror})`);
      }
    });
  }
} catch(error) {
  results.push({name:"Harness setup",status:"FAIL",error:error.stack??error.message});
  console.error(error);
} finally {
  if(original) await evaluate(`pc.view.mode='grid';pc.settings.setRememberSession(${original.remember});pc.settings.setProgressiveLoupe(${original.progressive});pc.settings.collapseBursts=${original.collapse};pc.settings.setPreviewQuality(${original.quality});pc.api.setPreviewQuality(${original.quality});pc.session.setMirrorMode(${original.mirror});pc.traceObserver?.disconnect();pc.stopProgress?.()`).catch(()=>{});
  await writeFile(join(output,"results.json"),JSON.stringify({date:new Date().toISOString(),target:target.url,fixture,count,baseline,original,results,observations,runtimeErrors,finalState:await state().catch(()=>null)},null,2));
  await screenshot("final").catch(()=>{});
  socket.close();
}
process.exitCode=results.some(result=>result.status==='FAIL')?1:0;
