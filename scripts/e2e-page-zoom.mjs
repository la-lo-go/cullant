import { mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";

// Failure modes: Ctrl+wheel scales the toolbar; native zoom keys scale a dialog;
// blocking browser zoom also blocks photo zoom, grid scrolling, or text input.
const output = resolve(process.argv[2] ?? ".playwright-mcp/page-zoom");
const targets = await (await fetch(`http://127.0.0.1:${process.env.CULLANT_CDP_PORT ?? 9222}/json/list`)).json();
const target = targets.find(page => page.type === "page" && page.url.includes("localhost:1420"));
if (!target) throw new Error("Start npm run tauri:debug with the disposable preview-cache-project.");
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
async function evaluate(expression) {
  const reply = await send("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result.value;
}
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
async function waitFor(expression) {
  for (let attempt = 0; attempt < 120; attempt++) { if (await evaluate(expression).catch(() => false)) return; await pause(100); }
  throw new Error("Timed out: " + expression);
}
function assert(condition, message) { if (!condition) throw new Error(message); }
async function screenshot(name) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  await writeFile(join(output, name + ".png"), Buffer.from(shot.data, "base64"));
}
async function key(key, code, windowsVirtualKeyCode, modifiers = 0) {
  await send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode, modifiers });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode, modifiers });
  await pause(150);
}
async function wheel(selector, modifiers = 2, deltaY = -120) {
  const point = await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2}})()`);
  await send("Input.dispatchMouseEvent", { type: "mouseWheel", ...point, deltaY, deltaX: 0, modifiers });
  await pause(250);
}
async function pinch(selector) {
  const center = await evaluate(`(()=>{const r=document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2}})()`);
  const points = distance => [-1, 1].map((direction, id) => ({ id, x: center.x + direction * distance, y: center.y }));
  await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: points(30) });
  await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: points(100) });
  await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await pause(250);
}
const viewport = () => evaluate("({width:innerWidth,height:innerHeight,dpr:devicePixelRatio,scale:visualViewport.scale})");
async function check(name, run) {
  try { await run(); results.push({ name, status: "PASS" }); }
  catch (error) { results.push({ name, status: "FAIL", error: error.message }); await screenshot("failure-" + results.length).catch(() => {}); }
  console.log(`${results.at(-1).status}: ${name}${results.at(-1).error ? ": " + results.at(-1).error : ""}`);
}
await mkdir(output, { recursive: true });
let saved;
try {
  await send("Page.reload");
  await waitFor("performance.getEntriesByType('resource').some(entry=>new URL(entry.name).pathname==='/src/lib/stores/catalog.svelte.ts')");
  await evaluate(`(async()=>{
    const loaded=path=>import(performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname===path)?.name??path);
    const {catalog}=await loaded('/src/lib/stores/catalog.svelte.ts');
    const {session}=await loaded('/src/lib/stores/session.svelte.ts');
    const {view}=await loaded('/src/lib/stores/view.svelte.ts');
    const {settings}=await loaded('/src/lib/stores/settings.svelte.ts');
    window.pz={catalog,session,view,settings};
    if(!catalog.project)await catalog.open(${JSON.stringify(resolve(".playwright-mcp/preview-cache-project"))});
    if(!catalog.project.rootPath.replaceAll('\\\\','/').toLowerCase().endsWith('/.playwright-mcp/preview-cache-project'))throw new Error('Use only the disposable preview-cache-project.');
  })()`);
  await waitFor("pz.catalog.items.length>=8 && !pz.catalog.ingesting");
  saved = await evaluate("({storage:{...localStorage},mode:pz.view.mode,focus:pz.session.focusedIndex})");
  await evaluate("pz.settings.setRememberSession(false);pz.session.clearFilters();pz.session.clearSelection();pz.session.focusedIndex=0;pz.view.mode='grid';document.activeElement?.blur()");
  await check("Ctrl+wheel over the toolbar keeps the page scale", async () => {
    const before = await viewport();
    await wheel(".toolbar");
    const after = await viewport(); observations.push({ case: "toolbar-wheel", before, after });
    assert(JSON.stringify(before) === JSON.stringify(after), "Ctrl+wheel changed the page viewport");
  });
  await check("Browser zoom keys keep the grid and Settings at the same scale", async () => {
    const before = await viewport();
    for (const [value, code, number] of [["+", "Equal", 187], ["-", "Minus", 189], ["0", "Digit0", 48]]) await key(value, code, number, 2);
    await evaluate("document.querySelector('button[aria-label=Settings],button[title=Settings]').click()");
    await pause(100);
    for (const [value, code, number] of [["+", "Equal", 187], ["-", "Minus", 189]]) await key(value, code, number, 2);
    const after = await viewport(); observations.push({ case: "zoom-keys", before, after });
    assert(JSON.stringify(before) === JSON.stringify(after), "Browser zoom keys changed the page viewport");
    await key("Escape", "Escape", 27);
  });
  await check("Text input stays usable while native zoom is blocked", async () => {
    await evaluate("document.querySelector('button[aria-label=Settings],button[title=Settings]').click()");
    await pause(100);
    await evaluate("document.querySelector('[role=dialog] input[type=search]').focus()");
    const before = await viewport();
    await key("+", "Equal", 187, 2);
    assert(JSON.stringify(before) === JSON.stringify(await viewport()), "Native zoom changed scale with text input focused");
    await send("Input.insertText", { text: "zoom" });
    assert(await evaluate("document.querySelector('[role=dialog] input[type=search]').value==='zoom'"), "The zoom guard blocked text input");
    await key("Escape", "Escape", 27);
  });
  await check("Photo Ctrl+wheel and keyboard zoom still work without scaling the page", async () => {
    await evaluate("pz.session.focusedIndex=0;pz.view.mode='viewer';document.activeElement?.blur()");
    await waitFor("document.querySelector('.frame img')?.naturalWidth>0");
    const before = await viewport();
    await wheel(".frame");
    assert(await evaluate("document.querySelector('.frame').classList.contains('zoomed')"), "Ctrl+wheel no longer zooms the photo");
    assert(JSON.stringify(before) === JSON.stringify(await viewport()), "Photo wheel zoom also scaled the page");
    await key("z", "KeyZ", 90);
    await waitFor("!document.querySelector('.frame').classList.contains('zoomed')");
    await key("+", "Equal", 187, 2);
    assert(await evaluate("document.querySelector('.frame').classList.contains('zoomed')"), "The configured keyboard photo zoom stopped working");
    assert(JSON.stringify(before) === JSON.stringify(await viewport()), "Photo keyboard zoom also scaled the page");
    await screenshot("photo-zoom");
  });
  await check("Touch page zoom is disabled and image gestures retain their own touch area", async () => {
    const values = await evaluate("({viewport:document.querySelector('meta[name=viewport]').content,pageTouch:getComputedStyle(document.body).touchAction,imageTouch:getComputedStyle(document.querySelector('.frame')).touchAction})");
    observations.push({ case: "touch-policy", values });
    assert(values.viewport.includes("maximum-scale=1") && values.pageTouch === "pan-x pan-y" && values.imageTouch === "none", "The page permits native pinch zoom or blocks the custom image touch area");
  });
  await check("Two-finger pinch zooms the photo and keeps the toolbar scale", async () => {
    await key("z", "KeyZ", 90);
    await waitFor("!document.querySelector('.frame').classList.contains('zoomed')");
    await send("Emulation.setTouchEmulationEnabled", { enabled: true, maxTouchPoints: 2 });
    const before = await viewport();
    await pinch(".toolbar");
    assert(JSON.stringify(before) === JSON.stringify(await viewport()), "Two fingers zoomed the page over the toolbar");
    await pinch(".frame");
    assert(await evaluate("document.querySelector('.frame').classList.contains('zoomed')"), "The page guard blocked photo pinch zoom");
    assert(JSON.stringify(before) === JSON.stringify(await viewport()), "Photo pinch zoom changed the page scale");
    await screenshot("photo-pinch");
  });
} finally {
  await send("Emulation.setTouchEmulationEnabled", { enabled: false }).catch(() => {});
  if (saved) await evaluate(`pz.view.mode=${JSON.stringify(saved.mode)};pz.session.focusedIndex=${saved.focus};localStorage.clear();for(const[key,value]of Object.entries(${JSON.stringify(saved.storage)}))localStorage.setItem(key,value)`).catch(() => {});
  await writeFile(join(output, "results.json"), JSON.stringify({ generatedAt: new Date().toISOString(), results, observations }, null, 2) + "\n");
  socket.close();
}
process.exitCode = results.some(result => result.status === "FAIL") ? 1 : 0;
