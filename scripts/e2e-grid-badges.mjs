import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const output = resolve(process.argv[2] ?? ".playwright-mcp/grid-badges");
const targets = await (await fetch(`http://127.0.0.1:${process.env.CULLANT_CDP_PORT ?? 9222}/json/list`)).json();
const target = targets.find(page => page.type === "page" && page.url.includes("localhost:1420"));
if (!target) throw new Error("Start the debug app with the disposable preview-cache-project.");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
const pending = new Map(), results = [], observations = [];
let sequence = 0;
socket.onmessage = async ({ data }) => {
  const reply = JSON.parse(data);
  if (reply.method === "Fetch.requestPaused") {
    const id = Number(new URL(reply.params.request.url).pathname.split("/").at(-1));
    const portrait = id >= 10004 && id <= 10005;
    const image = `<svg xmlns="http://www.w3.org/2000/svg" width="${portrait ? 400 : id === 10010 ? 1200 : 600}" height="${portrait ? 600 : 400}"><defs><linearGradient id="sky" x2="1" y2="1"><stop stop-color="#8ac8ed"/><stop offset="1" stop-color="#2a5278"/></linearGradient></defs><path fill="url(#sky)" d="M0 0h1200v600H0z"/><path fill="#e6d7a5" d="M0 250 180 100 450 400H0z"/></svg>`;
    await send("Fetch.fulfillRequest", { requestId: reply.params.requestId, responseCode: 200,
      responseHeaders: [{ name: "Content-Type", value: "image/svg+xml" }], body: Buffer.from(image).toString("base64") });
    return;
  }
  const task = pending.get(reply.id);
  if (!task) return;
  pending.delete(reply.id);
  if (reply.error) task.reject(new Error(reply.error.message)); else task.resolve(reply.result);
};
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence; pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const reply = await send("Runtime.evaluate", { expression: `(async()=>{return await (${expression});})()`, awaitPromise: true, returnByValue: true });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result.value;
}
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function waitFor(expression) {
  for (let i = 0; i < 100; i++) {
    if (await evaluate(expression).catch(() => false)) return;
    await pause(50);
  }
  throw new Error(`Timed out: ${expression}`);
}
async function screenshot(name) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  await writeFile(join(output, `${name}.png`), Buffer.from(shot.data, "base64"));
}
await mkdir(output, { recursive: true });
try {
  await send("Runtime.enable");
  if (process.env.CULLANT_UI_URL) await send("Page.navigate", { url: process.env.CULLANT_UI_URL });
  else await send("Page.reload");
  await waitFor("performance.getEntriesByType('resource').some(entry=>new URL(entry.name).pathname==='/src/lib/stores/catalog.svelte.ts')");
  await evaluate(`(async()=>{
    const loaded=path=>import(performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname===path)?.name??path);
    const {catalog}=await loaded('/src/lib/stores/catalog.svelte.ts');
    const {session}=await loaded('/src/lib/stores/session.svelte.ts');
    const {settings}=await loaded('/src/lib/stores/settings.svelte.ts');
    const {view}=await loaded('/src/lib/stores/view.svelte.ts');
    const {folders}=await loaded('/src/lib/stores/folders.svelte.ts');
    if(catalog.project && !catalog.project.rootPath.replaceAll('\\\\','/').toLowerCase().includes('/.playwright-mcp/'))throw new Error('Use a disposable project.');
    window.gridProbe={catalog,session,settings,view,folders,originalProject:catalog.project,originalRemember:settings.rememberSession,storage:{...localStorage}};
    settings.rememberSession=false;
  })()`);
  await pause(250);
  await send("Fetch.enable", { patterns: [{ urlPattern: "*cullant.localhost/thumb/100*", requestStage: "Request" }] });
  await evaluate(`(()=>{
    const {catalog,session,settings,view,folders}=gridProbe;
    const base={relPath:'fixture.jpg',sourceVersion:'',lens:null,iso:null,focalLength:null,fNumber:null,exposureTime:null,...JSON.parse(JSON.stringify(catalog.items[0]??{}))};
    gridProbe.originalItems=JSON.parse(JSON.stringify(catalog.items));
    gridProbe.originalMedia=catalog.media;
    gridProbe.originalCounts={...catalog.mediaCounts};gridProbe.originalError=catalog.error;
    gridProbe.originalLoaded=[...catalog.thumbLoaded];gridProbe.originalReady=[...catalog.previewReady];
    gridProbe.originalSettings={gridPhotoFit:settings.gridPhotoFit,collapseBursts:settings.collapseBursts,burstMode:settings.burstMode,burstGapSeconds:settings.burstGapSeconds,dimQueuedDeletes:settings.dimQueuedDeletes};
    gridProbe.originalSession={gridDensity:session.gridDensity,mirrorMode:session.mirrorMode,showNames:session.showNames,groupBy:[...session.groupBy]};
    catalog.project={rootPath:${JSON.stringify(resolve(".playwright-mcp/preview-cache-project"))},displayName:'Grid badge preview',dbPath:'',schemaVersion:12,fileCount:10};
    catalog.error='';
    session.clearFilters();session.clearFocus();session.groupBy=[];session.mirrorMode=true;session.showNames=true;
    folders.selectOnly(null);view.mode='grid';catalog.media='photos';
    settings.collapseBursts=true;settings.burstMode='fixed';settings.burstGapSeconds=2;settings.dimQueuedDeletes=false;
    const row=(id,name,kind,groupId,time,flag=0)=>({...base,id,name,kind,groupId,groupSize:kind===0?2:1,isPrimary:true,ext:kind===0?'raf':'jpg',captureTime:time,mtime:time,camera:'Fixture',width:600,height:400,orientation:1,phash:'0000000000000000',flag,rating:5,label:'Blue',tagIds:[1,2,3,4],decoupled:false,thumbReady:true,thumbFailed:false,previewFailed:false});
    const rows=[row(10001,'raw-pair',0,20001,100),row(10002,'raw-pair',1,20001,100),row(10003,'raw-only',0,20003,200),row(10004,'burst-portrait',0,20004,300,1),row(10005,'burst-portrait-next',0,20005,301,-1),row(10006,'jpeg-burst',1,20006,400,1),row(10007,'jpeg-burst-next',1,20007,401,-1)];
    rows[1].isPrimary=false;rows[1].groupSize=2;rows[2].groupSize=1;rows[3].groupSize=1;rows[4].groupSize=1;
    rows[3].width=400;rows[3].height=600;rows[4].width=400;rows[4].height=600;
    rows[0].width=500;rows[2].width=500;
    const unknown=row(10008,'raw-unknown-dimensions',0,20008,500);unknown.width=null;unknown.height=null;unknown.groupSize=1;
    const rotated=row(10009,'raw-rotated',0,20009,600);rotated.width=400;rotated.height=600;rotated.orientation=6;rotated.groupSize=1;
    const wide=row(10010,'wide-photo',1,20010,700);wide.width=1200;
    rows.push(unknown,rotated,wide);
    catalog.items=rows;catalog.mediaCounts={photos:rows.length,videos:0};
    for(const row of rows)catalog.previewReady.add(row.id);
  })()`);
  for (const [width, density, fit, variant = "normal"] of [[390,"small","fit"],[390,"medium","fit"],[390,"large","fit"],[390,"small","fill"],[1280,"medium","fit"],[390,"small","fit","selected"],[390,"medium","fit","diverged"],[390,"medium","fit","split"]]) {
    const name = `${width}-${density}-${fit}-${variant}`;
    await send("Emulation.setDeviceMetricsOverride", { width, height: 844, deviceScaleFactor: 1, mobile: false });
    await evaluate(`(()=>{const {session,settings,catalog}=gridProbe;session.gridDensity=${JSON.stringify(density)};settings.gridPhotoFit=${JSON.stringify(fit)};session.selectedIds=new Set(${variant === "selected" ? "[10001,10004]" : "[]"});catalog.items[0].decoupled=${variant === "split"};catalog.items[1].flag=${variant === "diverged" ? "1" : "0"};return true;})()`);
    await waitFor("document.querySelectorAll('.cell .photo img').length===7 && [...document.querySelectorAll('.cell .photo img')].every(img=>img.complete && img.naturalWidth>0)");
    await pause(150);
    const metrics = await evaluate(`(()=>{
      const rect=el=>{const r=el.getBoundingClientRect();return{x:r.x,y:r.y,w:r.width,h:r.height,right:r.right,bottom:r.bottom};};
      return [...document.querySelectorAll('.cell')].map(cell=>{
        const photo=cell.querySelector('.photo'),image=photo.querySelector('img'),top=photo.querySelector('.info-top'),bottom=photo.querySelector('.info-bottom');
        return{name:cell.querySelector('.name').textContent,photo:rect(photo),image:rect(image),natural:{w:image.naturalWidth,h:image.naturalHeight},top:rect(top),bottom:rect(bottom),radius:getComputedStyle(photo).borderRadius,overflow:getComputedStyle(photo).overflow,
          badges:[...top.children].filter(el=>el.offsetHeight>0).map(el=>({text:el.textContent.trim(),rect:rect(el)})),
          counts:[...photo.querySelectorAll('.counts > span')].filter(el=>el.offsetHeight>0).map(rect),
          cornerIsImage:document.elementFromPoint(rect(photo).x+.2,rect(photo).y+.2)===image,
          type:photo.querySelector('.chip.pair,.chip.raw')?.offsetHeight?rect(photo.querySelector('.chip.pair,.chip.raw')):null};
      });
    })()`);
    observations.push({ name, metrics });
    const failures = [];
    if (variant === "diverged" && !await evaluate("document.querySelector('.chip.pair .pair-halves')?.textContent.replaceAll(/\\s/g,'')==='RAWJPG'")) failures.push("Diverged pair names are missing");
    if (variant === "split" && !await evaluate("document.querySelector('.chip.pair.split')?.textContent.trim()==='SPLIT'")) failures.push("Decoupled pair badge is missing");
    for (const row of metrics) {
      if (row.cornerIsImage) failures.push(`${row.name}: square image corner is visible`);
      if (fit === "fit" && Math.abs(row.photo.w/row.photo.h-row.natural.w/row.natural.h) > .03) failures.push(`${row.name}: rounded box and badges do not follow the visible thumbnail`);
      if (row.image.x < row.photo.x - 1 || row.image.right > row.photo.right + 1 || row.image.y < row.photo.y - 1 || row.image.bottom > row.photo.bottom + 1) failures.push(`${row.name}: image leaves its rounded box`);
      if (row.type && row.type.y > row.top.y + 1) failures.push(`${row.name}: type badge is pushed below the top row`);
      if (row.counts.length > 1 && Math.abs(row.counts[0].y - row.counts[1].y) > 1) failures.push(`${row.name}: pick/reject counts wrap vertically`);
      if (row.top.h > row.photo.h * .45) failures.push(`${row.name}: badges cover too much of the photo`);
      if (row.bottom.h && row.top.h && row.bottom.y < row.top.bottom-1) failures.push(`${row.name}: top and bottom badges overlap`);
      if (row.badges.some(badge=>badge.rect.x < row.photo.x || badge.rect.right > row.photo.right+.5 || badge.rect.bottom > row.photo.bottom+.5)) failures.push(`${row.name}: badge leaves its photo`);
    }
    results.push({ name, status: failures.length ? "FAIL" : "PASS", failures });
    await screenshot(name);
    console.log(`${results.at(-1).status}: ${name}${failures.length?`: ${failures.join('; ')}`:''}`);
  }
} finally {
  await evaluate(`(async()=>{
    if(!window.gridProbe)return;
    const {catalog,session,settings}=gridProbe;
    if(gridProbe.originalItems)catalog.items=gridProbe.originalItems;
    if(gridProbe.originalSettings)Object.assign(settings,gridProbe.originalSettings);
    if(gridProbe.originalSession)Object.assign(session,gridProbe.originalSession);
    if(gridProbe.originalMedia)catalog.media=gridProbe.originalMedia;
    if(gridProbe.originalCounts)catalog.mediaCounts=gridProbe.originalCounts;
    if(gridProbe.originalLoaded){catalog.thumbLoaded.clear();for(const id of gridProbe.originalLoaded)catalog.thumbLoaded.add(id);}
    if(gridProbe.originalReady){catalog.previewReady.clear();for(const id of gridProbe.originalReady)catalog.previewReady.add(id);}
    catalog.error=gridProbe.originalError??'';session.selectedIds=new Set();
    catalog.project=gridProbe.originalProject;
    settings.rememberSession=gridProbe.originalRemember;
    for(const key of Object.keys(localStorage))if(!(key in gridProbe.storage))localStorage.removeItem(key);
    for(const [key,value]of Object.entries(gridProbe.storage))localStorage.setItem(key,value);
  })()`).catch(error=>console.error(error.message));
  await send("Fetch.disable").catch(()=>{});
  await send("Emulation.clearDeviceMetricsOverride").catch(()=>{});
  if (process.env.CULLANT_UI_URL) await send("Page.navigate", { url: target.url }).catch(()=>{});
  await writeFile(join(output,"results.json"),JSON.stringify({fixture:"Real VirtualGrid with in-memory rows and intercepted thumbnail responses; no media files or database rows changed.",results,observations},null,2));
  socket.close();
}
if (results.some(result=>result.status==="FAIL"))process.exitCode=1;
