import { mkdir, writeFile, copyFile } from "node:fs/promises";
import { resolve, join } from "node:path";
import { execFileSync } from "node:child_process";

// Failure modes: initial decoder errors cover a valid poster; warnings wait for
// Play; play hides the poster before a frame; old errors affect the next clip;
// a failed preview hides a valid thumbnail; remux starts after exit.
const fixture = resolve(".playwright-mcp/video-first-open-project");
const otherFixture = resolve(".playwright-mcp/video-first-open-other-project");
const output = resolve(process.argv[2] ?? ".playwright-mcp/video-first-open");
if (process.argv.includes("--prepare")) {
  await mkdir(fixture, { recursive: true });
  for (const [file, codec, seconds] of [["01-mjpeg.mov", "mjpeg", "1"], ["02-mjpeg.mov", "mjpeg", "1"], ["03-h264.mp4", "libx264", "2"]]) {
    execFileSync("ffmpeg", ["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=1280x720:rate=24", "-t", seconds, "-an", "-c:v", codec, "-pix_fmt", "yuv420p", join(fixture, file)]);
  }
  console.log(fixture);
  process.exit(0);
}
const targets = await (await fetch("http://127.0.0.1:" + (process.env.CULLANT_CDP_PORT ?? 9222) + "/json/list")).json();
const target = targets.find(page => page.type === "page" && page.url.includes("localhost:1420"));
if (!target) throw new Error("Start npm run tauri:debug with the video-first-open-project fixture.");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let sequence = 0;
const pending = new Map(), results = [], observations = [];
let blockedPoster = false, blockThumb = false, holdRemux = false, rejectVideoRead = false, deferNative = false;
const heldRequests = [], heldNativeRequests = [], videoRequests = [];
let originalStorage;
socket.onmessage = ({ data }) => {
  const reply = JSON.parse(data), task = pending.get(reply.id);
  if(reply.method==="Network.requestWillBeSent" && new URL(reply.params.request.url).pathname.startsWith("/video/")) videoRequests.push(reply.params.request.url);
  if (reply.method === "Fetch.requestPaused") {
    const {requestId,request} = reply.params, url = new URL(request.url);
    if(deferNative && url.pathname==="/video/3" && reply.params.resourceType==="Media") {heldNativeRequests.push(requestId);return;}
    if(rejectVideoRead && url.pathname==="/video/3" && reply.params.resourceType!=="Media") {void send("Fetch.fulfillRequest",{requestId,responseCode:503,body:""});return;}
    if (holdRemux && url.pathname === "/video/3") {heldRequests.push(requestId);return;}
    const fail = blockedPoster && (url.pathname === "/preview/1" && !url.searchParams.has("posterRetry") || blockThumb && url.pathname === "/thumb/1");
    observations.push({case:"poster-request",url:request.url,blocked:fail});
    void send(fail ? "Fetch.fulfillRequest" : "Fetch.continueRequest", fail ? {requestId,responseCode:404,body:""} : {requestId});
  }
  if (!task) return;
  pending.delete(reply.id);
  if (reply.error) task.reject(new Error(reply.error.message)); else task.resolve(reply.result);
};
socket.onclose = () => { for (const task of pending.values()) task.reject(new Error("App connection closed")); pending.clear(); };
function send(method, params = {}) {
  return new Promise((resolve, reject) => { const id = ++sequence; pending.set(id, { resolve, reject }); socket.send(JSON.stringify({ id, method, params })); });
}
async function evaluate(fn, arg) {
  const reply = await send("Runtime.evaluate", { expression: "(" + fn.toString() + ")(" + JSON.stringify(arg ?? null) + ")", awaitPromise: true, returnByValue: true });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result.value;
}
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function waitFor(fn, timeout = 8000) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) { if (await evaluate(fn).catch(() => false)) return; await pause(50); }
  throw new Error("Timed out: " + fn.toString());
}
function assert(condition, message) { if (!condition) throw new Error(message); }
async function screenshot(name) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  await writeFile(join(output, name + ".png"), Buffer.from(shot.data, "base64"));
}
async function state() {
  return evaluate(() => {
    const poster = document.querySelector(".video-poster img"), fallback = document.querySelector(".fallback"), wrap = document.querySelector(".video-wrap");
    const video = document.querySelector("video.player"), rect = fallback?.getBoundingClientRect(), stage = wrap?.getBoundingClientRect();
    return {file: vf.session.focused?.relPath, poster: poster && {src:poster.currentSrc,loaded:poster.complete && poster.naturalWidth>0,width:poster.naturalWidth},
      fallback:fallback && {text:fallback.textContent.trim(),height:rect.height,stageHeight:stage.height,blur:getComputedStyle(fallback).backdropFilter},
      media:video && {error:video.error?.code,paused:video.paused,ready:video.readyState}, noPoster:!!document.querySelector(".no-poster")};
  });
}
async function check(name, run) {
  try { await run(); results.push({ name, status: "PASS" }); }
  catch (error) { results.push({ name, status: "FAIL", error: error.message }); await screenshot("failure-" + results.length); }
  const result = results.at(-1); console.log(result.status + ": " + name + (result.error ? ": " + result.error : ""));
}
async function pendingPlayback(cancel) {
  await evaluate(() => {vf.view.mode="grid";vf.failures?.clear();});
  await evaluate(() => {vf.session.focusedIndex=2;vf.view.mode="viewer";});
  await waitFor(() => document.querySelector("video.player")?.readyState>=1);
  holdRemux = true;
  await send("Fetch.enable", {patterns:[{urlPattern:"*cullant.localhost/video/3*"}]});
  try {
    await evaluate(() => {
      const video=document.querySelector("video.player"), play=video.play;
      video.play=function(){return this.src.startsWith("blob:")?play.call(this):Promise.reject(new DOMException("Native decoder rejected the clip","NotSupportedError"));};
      video.dispatchEvent(new Event("error"));
    });
    const deadline=Date.now()+8000;
    while(!heldRequests.length && Date.now()<deadline) await pause(20);
    assert(heldRequests.length>0,"The remux metadata request did not start");
    await evaluate(cancel => {
      const play=document.querySelector(".video-wrap .controls .ctl");
      play.click();if(cancel)play.click();
    }, cancel);
  } finally {
    holdRemux = false;
    for(const requestId of heldRequests.splice(0)) await send("Fetch.continueRequest",{requestId});
    await send("Fetch.disable");
  }
  await waitFor(() => document.querySelector("video.player")?.src.startsWith("blob:") && document.querySelector("video.player")?.readyState>=1);
}
await mkdir(output, { recursive: true });
try {
  await send("Runtime.enable");
  await send("Network.enable");
  await send("Network.setCacheDisabled", {cacheDisabled:true});
  await send("Page.reload");
  await waitFor(() => performance.getEntriesByType("resource").some(entry=>new URL(entry.name).pathname==="/src/lib/stores/catalog.svelte.ts"));
  originalStorage = await evaluate(() => ({...localStorage}));
  await evaluate(async fixture => {
    const loaded = path => import(performance.getEntriesByType("resource").find(entry => new URL(entry.name).pathname === path)?.name ?? path);
    const {catalog} = await loaded("/src/lib/stores/catalog.svelte.ts");
    const {session} = await loaded("/src/lib/stores/session.svelte.ts");
    const {view} = await loaded("/src/lib/stores/view.svelte.ts");
    const {settings} = await loaded("/src/lib/stores/settings.svelte.ts");
    const {folders} = await loaded("/src/lib/stores/folders.svelte.ts");
    const {api} = await loaded("/src/lib/api.ts");
    const playerSource=await(await fetch("/src/lib/components/VideoPlayer.svelte")).text();
    const failureImport=playerSource.match(/from "([^"]*\/video\/failures\.ts[^"]*)"/);
    const failures=failureImport?(await import(failureImport[1])).unplayableVideos:null;
    failures?.clear();
    window.vf={catalog,session,view,settings,folders,api,failures,original:{remember:settings.rememberSession,quality:settings.previewQuality,sort:catalog.sort,desc:catalog.sortDesc,collapse:settings.collapseBursts}};
    await catalog.open(fixture);
    settings.setRememberSession(false);settings.setPreviewQuality(2560);settings.collapseBursts=false;await api.setPreviewQuality(2560);
    catalog.sort="name";catalog.sortDesc=false;
    session.clearFilters();session.clearSelection();folders.selectOnly(null);view.mode="grid";
    await catalog.setMedia("videos");
  }, fixture);
  await waitFor(() => vf.catalog.items.length===3 && !vf.catalog.ingesting && !vf.session.restoring);
  await check("A grid tap opens paused and a later video tap starts playback", async () => {
    await evaluate(() => {vf.session.clearSelection();vf.session.focusedIndex=2;vf.view.mode="grid";});
    await waitFor(() => document.querySelectorAll(".grid-root .cell").length===3);
    const point=await evaluate(() => {const rect=document.querySelectorAll(".grid-root .cell")[2].getBoundingClientRect();return{x:rect.left+rect.width/2,y:rect.top+rect.height/2};});
    await send("Emulation.setTouchEmulationEnabled",{enabled:true});
    try {
      await send("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[point]});
      await send("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});
      await waitFor(() => document.querySelector("video.player")?.readyState>=1);
      await pause(200);
      const opened=await evaluate(() => {const v=document.querySelector("video.player");return{paused:v.paused,time:v.currentTime};});
      observations.push({case:"single-grid-tap",state:opened});
      assert(opened.paused && opened.time===0,"The grid's opening tap starts video playback");
      const surface=await evaluate(() => {const rect=document.querySelector("video.player").getBoundingClientRect();return{x:rect.left+rect.width/2,y:rect.top+rect.height/2};});
      await send("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[surface]});
      await send("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});
      await waitFor(() => document.querySelector("video.player")?.currentTime>0 && !document.querySelector(".video-poster"));
      await evaluate(() => document.querySelector("video.player").pause());
    } finally {await send("Emulation.setTouchEmulationEnabled",{enabled:false});}
  });
  await check("A playing event without a presented frame retains the poster", async () => {
    await evaluate(() => {vf.view.mode="grid";});
    await evaluate(() => {vf.session.focusedIndex=2;vf.view.mode="viewer";});
    await waitFor(() => document.querySelector("video.player")?.readyState>=1 && document.querySelector(".video-poster img")?.naturalWidth>=1280);
    await evaluate(() => {const v=document.querySelector("video.player");v.currentTime=v.duration;});
    await pause(100);
    await evaluate(() => document.querySelector("video.player").dispatchEvent(new Event("playing")));
    await pause(100);
    assert((await state()).poster?.loaded,"A spurious playing event removes the poster");
  });
  await evaluate(() => {vf.view.mode="grid";});
  await evaluate(() => vf.api.discardPreviews());
  await evaluate(() => { vf.session.focusedIndex=0;vf.view.mode="viewer"; });
  await waitFor(() => !!document.querySelector(".fallback"));
  await pause(400);
  observations.push({ case: "first-open", state: await state() });
  await screenshot("first-open");
  await check("First unsupported clip shows a sharp poster on entry", async () => {
    await waitFor(() => document.querySelector(".video-poster img")?.naturalWidth>=1280);
    const first = await state(); observations.push({ case: "first-poster-ready", state: first });
    assert(first.poster?.loaded && !first.noPoster, "First video replaces its available poster with an error icon");
  });
  await check("External warning appears before Play and keeps the poster clear", async () => {
    const first = await state();
    assert(first.media.paused && first.fallback?.text.includes("Open in external player"), "No external playback warning before Play");
    assert(first.fallback.height < first.fallback.stageHeight / 2 && first.fallback.blur === "none", "Decoder error covers and blurs the whole poster");
  });
  await evaluate(() => { vf.session.focusedIndex=1; });
  await waitFor(() => !!document.querySelector(".fallback") && document.querySelector(".video-poster img")?.naturalWidth>=1280);
  observations.push({ case: "next-clip", state: await state() });
  await screenshot("next-clip");
  await check("A play event alone cannot remove the poster", async () => {
    await evaluate(() => document.querySelector("video.player").dispatchEvent(new Event("play")));
    await pause(100);
    assert((await state()).poster?.loaded, "Poster disappears before the video presents a frame");
  });
  await evaluate(() => { vf.session.focusedIndex=2; });
  await waitFor(() => document.querySelector("video.player")?.readyState>=1);
  observations.push({ case: "playable-clip", state: await state() });
  await check("Playable clips have no external warning and retain the poster until playback", async () => {
    await waitFor(() => document.querySelector(".video-poster img")?.naturalWidth>=1280);
    assert(!(await state()).fallback, "Previous clip's failure leaks into a playable clip");
    await evaluate(() => document.querySelector("video.player").play());
    await waitFor(() => !document.querySelector(".video-poster") && document.querySelector("video.player")?.currentTime>0);
    await screenshot("playable-clip");
  });
  await check("A late playback error restores the poster and cancels further playback", async () => {
    await evaluate(() => document.querySelector("video.player").dispatchEvent(new Event("error")));
    await waitFor(() => document.querySelector("video.player")?.src.startsWith("blob:"));
    await waitFor(() => document.querySelector("video.player")?.readyState>=1);
    await evaluate(() => document.querySelector("video.player").play());
    await waitFor(() => !document.querySelector(".video-poster"));
    await evaluate(() => document.querySelector("video.player").dispatchEvent(new Event("error")));
    await waitFor(() => !!document.querySelector(".fallback") && document.querySelector(".video-poster img")?.naturalWidth>=1280);
    const late = await state(); observations.push({case:"late-remux-error",state:late});
    assert(late.media.paused, "A failed remux keeps playing");
    await evaluate(() => document.querySelector(".video-wrap").dispatchEvent(new KeyboardEvent("keydown",{key:" ",code:"Space",bubbles:true})));
    assert((await state()).poster?.loaded, "Space hides the poster after terminal failure");
    await screenshot("late-remux-error");
  });
  await evaluate(() => { vf.view.mode="grid"; });
  await check("A recoverable load error remuxes in app without autoplay or an external warning", async () => {
    await evaluate(() => vf.failures?.clear());
    await evaluate(() => { vf.session.focusedIndex=2;vf.view.mode="viewer"; });
    await waitFor(() => document.querySelector("video.player")?.readyState>=1);
    await evaluate(() => document.querySelector("video.player").dispatchEvent(new Event("error")));
    await waitFor(() => document.querySelector("video.player")?.src.startsWith("blob:") && document.querySelector("video.player")?.readyState>=1);
    await waitFor(() => document.querySelector(".video-poster img")?.naturalWidth>=1280);
    const recovered = await state(); observations.push({case:"remux-ready",state:recovered});
    assert(recovered.media.paused && !recovered.fallback && recovered.poster?.loaded, "Remux autoplays or shows a false failure");
    await evaluate(() => document.querySelector("video.player").play());
    await waitFor(() => !document.querySelector(".video-poster") && document.querySelector("video.player")?.currentTime>0);
    await screenshot("remux-playing");
  });
  await check("Play during a pending remux resumes when the new source is ready", async () => {
    await pendingPlayback(false);
    await waitFor(() => !document.querySelector(".video-poster") && document.querySelector("video.player")?.currentTime>0);
    assert(!(await state()).fallback, "Pending Play produces an external failure after a valid remux");
  });
  await check("A second Play action cancels playback while a remux is pending", async () => {
    await pendingPlayback(true);
    await pause(100);
    const canceled=await state();observations.push({case:"canceled-pending-play",state:canceled});
    assert(canceled.media.paused && canceled.poster && !canceled.fallback, "Canceled Play still autoplays the remux");
  });
  await evaluate(() => { vf.view.mode="grid"; });
  await check("Navigating during a remux prevents the old source from replacing the new clip", async () => {
    await evaluate(() => { vf.session.focusedIndex=2;vf.view.mode="viewer"; });
    await waitFor(() => document.querySelector("video.player")?.readyState>=1);
    await evaluate(() => {
      document.querySelector("video.player").dispatchEvent(new Event("error"));
      vf.session.focusedIndex=1;
    });
    await waitFor(() => !!document.querySelector(".fallback") && document.querySelector(".video-poster img")?.naturalWidth>=1280);
    await pause(300);
    const next = await state(); observations.push({case:"navigate-during-remux",state:next});
    assert(next.file==="02-mjpeg.mov" && !await evaluate(() => document.querySelector("video.player").src.startsWith("blob:")), "Old remux source leaks into the new clip");
  });
  await check("The external-player action targets the focused video", async () => {
    const opened = await evaluate(() => {
      const original = vf.api.openExternal, ids = [];
      vf.api.openExternal = async id => {ids.push(id);};
      try {document.querySelector(".fallback .open-ext").click();}
      finally {vf.api.openExternal=original;}
      return {ids,focused:vf.session.focused.id};
    });
    assert(opened.ids.length===1 && opened.ids[0]===opened.focused, "External action targets a stale or different clip");
  });
  await check("The warning and external action fit a narrow viewport", async () => {
    await send("Emulation.setDeviceMetricsOverride",{width:390,height:844,deviceScaleFactor:1,mobile:false});
    await pause(150);
    const narrow = await evaluate(() => {
      const banner=document.querySelector(".fallback").getBoundingClientRect(), button=document.querySelector(".fallback .open-ext").getBoundingClientRect(), controls=document.querySelector(".video-wrap .controls").getBoundingClientRect();
      return {width:innerWidth,banner:{left:banner.left,right:banner.right,bottom:banner.bottom},button:{left:button.left,right:button.right,bottom:button.bottom},controlsTop:controls.top};
    });
    observations.push({case:"narrow-warning",state:narrow});
    assert(narrow.button.left>=0 && narrow.button.right<=narrow.width && narrow.banner.bottom<=narrow.controlsTop, "External action leaves the viewport or overlaps the transport bar");
    await screenshot("narrow-warning");
  });
  await send("Emulation.clearDeviceMetricsOverride");
  await evaluate(() => { vf.view.mode="grid"; });
  await send("Fetch.enable", {patterns:[{urlPattern:"*cullant.localhost/preview/*"},{urlPattern:"*cullant.localhost/thumb/*"}]});
  blockedPoster = true;
  await check("A successful sharp-poster retry keeps the loaded retry URL", async () => {
    await evaluate(() => { vf.session.focusedIndex=0;vf.view.mode="viewer"; });
    await waitFor(() => document.querySelector(".video-poster img")?.naturalWidth>=1280);
    const retry = await state(); observations.push({case:"retry-with-thumb",state:retry});
    assert(retry.poster.src.includes("posterRetry=1"), "Poster discards the successful retry and loads the failed URL again");
    await screenshot("retry-with-thumb");
  });
  await evaluate(() => { vf.view.mode="grid"; });
  blockThumb = true;
  await check("A sharp-poster retry recovers the first clip when its thumbnail is unavailable", async () => {
    await evaluate(() => { vf.session.focusedIndex=0;vf.view.mode="viewer"; });
    await waitFor(() => document.querySelector(".video-poster img")?.naturalWidth>=1280);
    const retry = await state(); observations.push({case:"retry-without-thumb",state:retry});
    assert(!retry.noPoster, "A successful retry still shows the missing-poster icon");
    await screenshot("retry-without-thumb");
  });
  blockedPoster=false;blockThumb=false;await send("Fetch.disable");
  await check("Compatibility checks do not show a preparation notice", async () => {
    await evaluate(() => {vf.view.mode="grid";});
    await evaluate(() => {vf.session.focusedIndex=2;vf.view.mode="viewer";});
    await waitFor(() => document.querySelector("video.player")?.readyState>=1);
    holdRemux=true;await send("Fetch.enable",{patterns:[{urlPattern:"*cullant.localhost/video/3*"}]});
    try {
      await evaluate(() => document.querySelector("video.player").dispatchEvent(new Event("error")));
      const deadline=Date.now()+8000;
      while(!heldRequests.length && Date.now()<deadline) await pause(20);
      assert(heldRequests.length>0,"The compatibility check did not read the video");
      assert(!await evaluate(() => !!document.querySelector(".remuxing")),"The preparation notice appears before compatibility is known");
    } finally {
      holdRemux=false;for(const requestId of heldRequests.splice(0)) await send("Fetch.continueRequest",{requestId});
      await send("Fetch.disable");
    }
    await waitFor(() => document.querySelector("video.player")?.src.startsWith("blob:") && document.querySelector("video.player")?.readyState>=1);
  });
  await evaluate(() => {vf.view.mode="grid";});
  await evaluate(() => {vf.session.focusedIndex=0;vf.view.mode="viewer";});
  await waitFor(() => !!document.querySelector(".fallback"));
  await check("Every transport control is disabled after failure", async () => {
    const disabled=await evaluate(() => ({controls:[...document.querySelectorAll(".video-wrap .controls button,.video-wrap .controls input")].map(control=>({title:control.title,disabled:control.disabled})),externalEnabled:!document.querySelector(".fallback .open-ext").disabled}));
    observations.push({case:"disabled-transport",state:disabled});
    assert(disabled.controls.every(control=>control.disabled),"A transport control remains active after failure");
    assert(disabled.externalEnabled,"The warning's external action is disabled");
  });
  await check("The failure warning has no Learn more action", async () => {
    assert(!await evaluate(() => !!document.querySelector(".fallback [data-info-tip]")),"The removed Learn more action remains visible");
  });
  await check("A confirmed failure reopens immediately without video requests", async () => {
    const before=videoRequests.length;
    await evaluate(() => {vf.view.mode="grid";});
    await evaluate(() => {vf.session.focusedIndex=0;vf.view.mode="viewer";});
    await waitFor(() => !!document.querySelector(".fallback"));await pause(150);
    assert(videoRequests.length===before,"Reopening a confirmed failure still reads the video");
    await screenshot("remembered-failure");
  });
  await check("A rejected codec never shows preparation", async () => {
    await evaluate(() => {vf.view.mode="grid";});
    await evaluate(() => {vf.session.focusedIndex=2;vf.view.mode="viewer";});
    await waitFor(() => document.querySelector("video.player")?.readyState>=1);
    await evaluate(() => {
      window.originalVideoSupport=MediaSource.isTypeSupported;MediaSource.isTypeSupported=()=>false;
      window.preparationNotices=[];window.preparationObserver=new MutationObserver(()=>{if(document.querySelector(".remuxing"))preparationNotices.push(true);});
      preparationObserver.observe(document.querySelector(".video-wrap"),{childList:true,subtree:true});
      document.querySelector("video.player").dispatchEvent(new Event("error"));
    });
    try {
      await waitFor(() => !!document.querySelector(".fallback"));
      assert(await evaluate(() => preparationNotices.length===0),"The unsupported codec briefly shows preparation");
    } finally {await evaluate(() => {MediaSource.isTypeSupported=originalVideoSupport;preparationObserver.disconnect();vf.view.infoTip=null;});}
  });
  await check("A changed file version retries playback", async () => {
    const mtime=await evaluate(() => vf.catalog.items.find(item=>item.id===3).mtime);
    try {
      await evaluate(mtime => {vf.catalog.items=vf.catalog.items.map(item=>item.id===3?{...item,mtime:mtime+1}:item);},mtime);
      await waitFor(() => document.querySelector("video.player")?.readyState>=1 && !document.querySelector(".fallback"));
    } finally {await evaluate(mtime => {vf.view.mode="grid";vf.catalog.items=vf.catalog.items.map(item=>item.id===3?{...item,mtime}:item);},mtime);}
  });
  await check("Switching projects clears the previous clip failure", async () => {
    await evaluate(() => {vf.session.focusedIndex=0;vf.view.mode="viewer";});
    await waitFor(() => !!document.querySelector(".fallback"));
    await mkdir(otherFixture,{recursive:true});await copyFile(join(fixture,"03-h264.mp4"),join(otherFixture,"01-h264.mp4"));
    await evaluate(async fixture => {await vf.catalog.open(fixture);vf.session.clearFilters();vf.folders.selectOnly(null);await vf.catalog.setMedia("videos");},otherFixture);
    await waitFor(() => vf.catalog.items.length===1 && !vf.catalog.ingesting);
    await evaluate(() => {vf.session.focusedIndex=0;vf.view.mode="viewer";});
    await waitFor(() => document.querySelector("video.player")?.readyState>=1 && !document.querySelector(".fallback"));
  });
  await check("A temporary read failure can retry without restarting the app", async () => {
    await evaluate(async fixture => {vf.view.mode="grid";vf.failures?.clear();await vf.catalog.open(fixture);await vf.catalog.setMedia("videos");vf.session.focusedIndex=2;vf.view.mode="viewer";},fixture);
    await waitFor(() => document.querySelector("video.player")?.readyState>=1);
    rejectVideoRead=true;await send("Fetch.enable",{patterns:[{urlPattern:"*cullant.localhost/video/3*"}]});
    try {
      await evaluate(() => document.querySelector("video.player").dispatchEvent(new Event("error")));
      await waitFor(() => !!document.querySelector(".fallback"));
    } finally {rejectVideoRead=false;await send("Fetch.disable");}
    await evaluate(() => {vf.view.mode="grid";});
    await evaluate(() => {vf.session.focusedIndex=2;vf.view.mode="viewer";});
    await waitFor(() => document.querySelector("video.player")?.readyState>=1 && !document.querySelector(".fallback"));
  });
  await check("An unsupported clip warns without a native error or another tap", async () => {
    await evaluate(() => {vf.view.mode="grid";vf.failures?.clear();});
    await evaluate(() => {
      window.originalVideoSupport=MediaSource.isTypeSupported;MediaSource.isTypeSupported=()=>false;
      window.originalNativeSupport=HTMLMediaElement.prototype.canPlayType;HTMLMediaElement.prototype.canPlayType=()=>"";
      window.preparationNotices=[];window.preparationObserver=new MutationObserver(()=>{if(document.querySelector(".remuxing"))preparationNotices.push(true);});
      preparationObserver.observe(document.body,{childList:true,subtree:true});
    });
    deferNative=true;await send("Fetch.enable",{patterns:[{urlPattern:"*cullant.localhost/video/3*"}]});
    try {
      await evaluate(() => {vf.session.focusedIndex=2;vf.view.mode="viewer";});
      await waitFor(() => !!document.querySelector(".fallback"),4000);
      const opened=await state();observations.push({case:"deferred-native-error",state:opened});
      assert(opened.media.paused && !opened.media.error,"The warning required a native playback error");
      assert(await evaluate(() => preparationNotices.length===0),"The unsupported clip briefly shows preparation");
      await screenshot("deferred-native-error");
    } finally {
      await evaluate(() => {vf.view.mode="grid";MediaSource.isTypeSupported=originalVideoSupport;HTMLMediaElement.prototype.canPlayType=originalNativeSupport;preparationObserver.disconnect();});
      deferNative=false;
      for(const requestId of heldNativeRequests.splice(0)) await send("Fetch.failRequest",{requestId,errorReason:"Aborted"}).catch(()=>{});
      await send("Fetch.disable");
    }
  });
} catch (error) {
  results.push({ name: "Harness setup", status: "FAIL", error: error.stack ?? error.message });
  console.error(error);
} finally {
  await send("Fetch.disable").catch(() => {});
  await send("Network.setCacheDisabled", {cacheDisabled:false}).catch(() => {});
  await send("Emulation.clearDeviceMetricsOverride").catch(() => {});
  await evaluate(async storage => {
    if (!window.vf?.original) return;
    vf.settings.setRememberSession(vf.original.remember);vf.settings.setPreviewQuality(vf.original.quality);
    vf.catalog.sort=vf.original.sort;vf.catalog.sortDesc=vf.original.desc;vf.settings.collapseBursts=vf.original.collapse;
    await vf.api.setPreviewQuality(vf.original.quality);
    for(const key of Object.keys(localStorage)) if(key.startsWith("cullant.")) localStorage.removeItem(key);
    for(const [key,value] of Object.entries(storage)) localStorage.setItem(key,value);
  }, originalStorage).catch(() => {});
  await writeFile(join(output, "results.json"), JSON.stringify({ date: new Date().toISOString(), target: target.url, results, observations }, null, 2));
  socket.close();
}
process.exitCode = results.some(result => result.status === "FAIL") ? 1 : 0;
