import { mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";

// Check submenu navigation, key recording, empty grid clicks, scrolls and captions.
const output = resolve(process.argv[2] ?? ".playwright-mcp/settings-photo-ui");
const targets = await (await fetch(`http://127.0.0.1:${process.env.CULLANT_CDP_PORT ?? 9222}/json/list`)).json();
const target = targets.find((page) => page.type === "page" && page.url.includes("localhost:1420"));
if (!target) throw new Error("Start the debug app with the disposable preview-cache-project.");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let sequence = 0;
const pending = new Map(), results = [], observations = [], runtimeErrors = [];
socket.onmessage = ({ data }) => {
  const reply = JSON.parse(data), task = pending.get(reply.id);
  if (reply.method === "Runtime.exceptionThrown") runtimeErrors.push(reply.params);
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
  const reply = await send("Runtime.evaluate", { expression: `(async()=>{const result=await eval(${JSON.stringify(expression)});return result===undefined?null:JSON.parse(JSON.stringify(result));})()`, awaitPromise: true, returnByValue: true });
  if (reply.exceptionDetails) throw new Error(reply.exceptionDetails.exception?.description ?? reply.exceptionDetails.text);
  return reply.result.value;
}
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const settle = () => evaluate("new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))");
function assert(condition, message) { if (!condition) throw new Error(message); }
async function waitFor(expression) {
  for (let i = 0; i < 100; i++) { if (await evaluate(expression).catch(() => false)) return; await pause(50); }
  throw new Error(`Timed out: ${expression}`);
}
async function screenshot(name) {
  const shot = await send("Page.captureScreenshot", { format: "png" });
  await writeFile(join(output, `${name}.png`), Buffer.from(shot.data, "base64"));
}
async function check(name, run) {
  try { await run(); results.push({ name, status: "PASS" }); }
  catch (error) { results.push({ name, status: "FAIL", error: error.message }); await screenshot(`failure-${results.length}`).catch(() => {}); }
  console.log(`${results.at(-1).status}: ${name}${results.at(-1).error ? `: ${results.at(-1).error}` : ""}`);
}
async function point(selector) {
  return evaluate(`(()=>{const el=document.querySelector(${JSON.stringify(selector)});if(!el)throw new Error('Missing '+${JSON.stringify(selector)});el.scrollIntoView({block:'nearest'});const r=el.getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2};})()`);
}
async function clickAt(point, modifiers = 0) {
  await send("Input.dispatchMouseEvent", { type: "mousePressed", ...point, button: "left", clickCount: 1, modifiers });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", ...point, button: "left", clickCount: 1, modifiers }); await settle();
}
const click = async (selector) => clickAt(await point(selector));
async function key(key, code, windowsVirtualKeyCode) {
  await send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode }); await settle();
}
async function settingsRoot() {
  await evaluate("document.querySelector('.dialog .close-x')?.click();ui.view.settingsPanel=null;ui.view.shortcutsOpen=false;document.querySelector('button[title=Settings]')?.click()");
  await waitFor("!!document.querySelector('[role=dialog][aria-label=Settings]')");
}
async function openSubmenu(title) {
  await settingsRoot();
  await evaluate(`[...document.querySelectorAll('.dialog .jump .wide')].find(el=>el.textContent.includes(${JSON.stringify(title)}))?.click()`); await settle();
}
await mkdir(output, { recursive: true });
try {
  await send("Runtime.enable"); await send("Page.reload");
  await waitFor("performance.getEntriesByType('resource').some(entry=>new URL(entry.name).pathname==='/src/lib/stores/catalog.svelte.ts')");
  await evaluate(`(async()=>{
    const loaded=path=>import(performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname===path)?.name??path);
    const {catalog}=await loaded('/src/lib/stores/catalog.svelte.ts');const {session}=await loaded('/src/lib/stores/session.svelte.ts');
    const {settings}=await loaded('/src/lib/stores/settings.svelte.ts');const {view}=await loaded('/src/lib/stores/view.svelte.ts');const {folders}=await loaded('/src/lib/stores/folders.svelte.ts');
    const {keymap}=await loaded('/src/lib/keyboard/dispatcher.svelte.ts');const {tags}=await loaded('/src/lib/stores/tags.svelte.ts');
    if(!catalog.project)await catalog.open(${JSON.stringify(resolve(".playwright-mcp/preview-cache-project"))});
    if(!catalog.project?.rootPath.replaceAll('\\\\','/').toLowerCase().endsWith('/.playwright-mcp/preview-cache-project'))throw new Error('Use only the disposable preview-cache-project.');
    window.ui={catalog,session,settings,view,folders,keymap,tags,project:catalog.project,items:JSON.parse(JSON.stringify(catalog.items)),storage:{...localStorage}};
    settings.setRememberSession(false);settings.setCollapseBursts(false);session.clearFilters();session.clearFocus();folders.selectOnly(null);view.mode='grid';
  })()`);
  await waitFor("ui.catalog.items.length>=8 && !ui.catalog.ingesting");
  await evaluate("ui.items=JSON.parse(JSON.stringify(ui.catalog.items))");
  await check("Configured-shortcuts help belongs to the keyboard submenu", async () => {
    await settingsRoot();
    assert(!await evaluate("document.querySelector('[aria-label=Settings]').textContent.includes('Show configured shortcuts')"), "Settings still shows configured-shortcuts help");
    await openSubmenu("Keyboard shortcuts");
    assert(await evaluate("document.querySelector('.dialog').textContent.includes('Show configured shortcuts')"), "Keyboard submenu does not show configured-shortcuts help");
  });
  await check("All settings submenus use one panel and support Back and Close", async () => {
    const styles = [];
    for (const title of ["Action bar", "Radial menu", "Keyboard shortcuts", "Task tags"]) {
      await openSubmenu(title);
      const row = await evaluate("(()=>{const el=document.querySelector('.dialog'),s=getComputedStyle(el);return{shared:el.classList.contains('settings-panel'),title:el.querySelector('h2')?.textContent,background:s.backgroundColor,border:s.borderRadius,back:!!el.querySelector('[aria-label=Back]'),close:!!el.querySelector('.close-x')};})()");
      styles.push({ title, ...row }); assert(row.shared && row.back && row.close && row.title === title, `${title} has an old panel or missing navigation`);
      await click(".dialog [aria-label=Back]");
      assert(await evaluate("document.querySelector('.dialog h2')?.textContent==='Settings'"), `${title} Back does not return to settings`);
    }
    observations.push({ case: "submenu-panels", styles });
    assert(new Set(styles.map((row) => `${row.background}|${row.border}`)).size === 1, "Panel styles differ");
    await openSubmenu("Keyboard shortcuts"); await click(".dialog .close-x");
    assert(!await evaluate("!!document.querySelector('.settings-panel')"), "Close returns to settings instead of closing");
  });
  await check("Escape cancels recording, then returns to settings", async () => {
    await openSubmenu("Keyboard shortcuts"); await click(".dialog .key");
    await key("Escape", "Escape", 27);
    assert(await evaluate("document.querySelector('.dialog h2')?.textContent==='Keyboard shortcuts' && ui.keymap.rebinding===null"), "Escape closes during recording");
    await key("Escape", "Escape", 27);
    assert(await evaluate("document.querySelector('.dialog h2')?.textContent==='Settings'"), "Escape does not return to settings"); await click(".dialog .close-x");
  });
  await check("Configured-shortcuts overlay returns to its submenu", async () => {
    await openSubmenu("Keyboard shortcuts"); await click(".show-shortcuts");
    assert(await evaluate("ui.view.shortcutsOpen"), "Configured shortcuts do not open"); await key("Escape", "Escape", 27);
    assert(await evaluate("!ui.view.shortcutsOpen && document.querySelector('.settings-panel h2')?.textContent==='Keyboard shortcuts'"), "The overlay does not return to its submenu"); await click(".settings-panel .close-x");
  });
  await check("Mouse clicks in gutters clear focus and arrows resume", async () => {
    await evaluate("document.querySelector('.dialog .close-x')?.click();ui.view.mode='grid';ui.session.groupBy=[];ui.session.selectOnly(0)"); await settle(); await pause(250);
    const empty = await evaluate("(()=>{const r=document.querySelector('.cell').getBoundingClientRect();return{x:r.left-2,y:r.top+r.height/2};})()");
    await clickAt(empty);
    assert(await evaluate("ui.session.focusedIndex===-1 && ui.session.selectedIds.size===0"), "A grid gutter still focuses a thumbnail");
    await key("ArrowRight", "ArrowRight", 39);
    assert(await evaluate("ui.session.focusedIndex===0"), "Arrows cannot resume after clearing focus");
    await click('[aria-label="Settings"]'); await click(".dialog .close-x");
    assert(await evaluate("ui.session.focusedIndex===0"), "A settings control clears image focus");
  });
  await check("Touch empty-space taps clear focus and scrolling preserves it", async () => {
    await send("Emulation.setTouchEmulationEnabled", { enabled: true });
    await evaluate("ui.view.mode='grid';ui.catalog.items=ui.items.slice(0,8);ui.session.selectOnly(0)"); await settle(); await pause(250);
    const empty = await evaluate("(()=>{const r=document.querySelector('.cell').getBoundingClientRect();return{x:r.left-2,y:r.top+r.height/2};})()");
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [empty] }); await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); await settle();
    assert(await evaluate("ui.session.focusedIndex===-1 && ui.view.mode==='grid'"), "A blank tap leaves focus or opens the viewer");
    await evaluate("ui.session.selectOnly(0)"); await settle();
    const start = await point(".cell");
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [start] });
    await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [{ ...start, y: start.y - 35 }] });
    await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); await settle();
    assert(await evaluate("ui.session.focusedIndex===0 && ui.view.mode==='grid'"), "A swipe clears focus or opens the viewer");
    await send("Emulation.setTouchEmulationEnabled", { enabled: false });
  });
  await check("Ctrl selection, drag selection and keyboard navigation remain usable", async () => {
    await evaluate("ui.view.mode='grid';ui.session.groupBy=[];ui.session.selectOnly(0)"); await settle(); await pause(250);
    await clickAt(await point(".cell:nth-of-type(2)"), 2);
    assert(await evaluate("ui.session.selectedIds.size===2 && ui.session.focusedIndex===-1"), "Ctrl selection lost its first item");
    await key("ArrowRight", "ArrowRight", 39);
    assert(await evaluate("ui.session.selectedIds.size===0 && ui.session.focusedIndex===0"), "Arrows leave a selection behind"); await pause(250);
    const [start, end] = await evaluate("(()=>{const cells=[...document.querySelectorAll('.cell')].slice(0,2).map(el=>el.getBoundingClientRect());return[{x:cells[0].left-2,y:cells[0].top+3},{x:cells[1].right-3,y:cells[1].bottom-3}];})()");
    await send("Input.dispatchMouseEvent", { type: "mousePressed", ...start, button: "left", clickCount: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", ...end, button: "left", buttons: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", ...end, button: "left", clickCount: 1 }); await settle();
    assert(await evaluate("ui.session.selectedIds.size===2 && ui.session.focusedIndex===-1"), "A marquee does not select both visible cells"); await screenshot("grid-selection");
  });
  await check("Group header clicks keep focus", async () => {
    await evaluate("ui.view.mode='grid';ui.session.groupBy=['folder'];ui.session.selectOnly(0)"); await settle(); await pause(250);
    const header = await point(".group-header .gh-label"); await clickAt(header);
    assert(await evaluate("ui.session.focusedIndex===0"), "A header label clears focus");
    await evaluate("ui.session.groupBy=[]"); await settle();
  });
  await check("Compare captions omit target and focus indicators", async () => {
    await evaluate("ui.session.selectOnly(0);ui.view.mode='compare'"); await settle();
    assert(!await evaluate("!!document.querySelector('.compare .target-status')"), "Compare still has target/focus filename indicators");
    assert(await evaluate("document.querySelectorAll('.compare .caption').length===2"), "Compare no longer shows both filenames"); await screenshot("compare");
  });
  await check("Project fans use a translucent border inside the image", async () => {
    await evaluate("ui.catalog.project=null;ui.view.mode='grid'");
    await waitFor("[...document.querySelectorAll('.gallery .thumbnail img')].some(img=>img.naturalWidth>0)");
    const border = await evaluate("(()=>{const img=[...document.querySelectorAll('.gallery .thumbnail img')].find(el=>el.naturalWidth>0),el=img.parentElement,s=getComputedStyle(el),edge=getComputedStyle(el,'::after');return{outer:s.borderWidth,inner:edge.borderWidth,color:edge.borderColor,inset:[edge.top,edge.right,edge.bottom,edge.left],width:el.clientWidth,imageWidth:img.clientWidth};})()");
    observations.push({ case: "fan-border", border });
    assert(border.outer === "0px" && border.inner === "2px" && border.color.includes("0.22") && border.inset.every((value) => value === "0px") && border.width === border.imageWidth, "The border hides image pixels or adds an outside margin");
    await screenshot("project-fans"); await evaluate("ui.catalog.project=ui.project"); await settle();
  });
  await screenshot("final");
} catch (error) { results.push({ name: "Harness setup", status: "FAIL", error: error.stack ?? error.message }); console.error(error); }
finally {
  await send("Emulation.setTouchEmulationEnabled", { enabled: false }).catch(() => {});
  await evaluate("(()=>{if(!window.ui)return;document.querySelector('.dialog .close-x')?.click();ui.catalog.project=ui.project;ui.catalog.items=ui.items;ui.session.groupBy=[];ui.view.mode='grid';ui.tags.editorOpen=false;ui.keymap.rebinding=null;for(const key of Object.keys(localStorage))if(key.startsWith('cullant.'))localStorage.removeItem(key);for(const [key,value]of Object.entries(ui.storage))localStorage.setItem(key,value);})()").catch(() => {});
  await writeFile(join(output, "results.json"), JSON.stringify({ date: new Date().toISOString(), target: target.url, results, observations, runtimeErrors }, null, 2));
  await send("Page.reload").catch(() => {}); socket.close();
}
process.exitCode = results.some((result) => result.status === "FAIL") ? 1 : 0;
