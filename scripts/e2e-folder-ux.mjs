import { mkdir, writeFile, unlink, utimes, rename } from "node:fs/promises";
import { resolve } from "node:path";

const output = process.argv[2] ?? ".playwright-mcp/folder-ux";
const fixture = resolve(".playwright-mcp/folder-ux-project");
if (process.argv[2] === "--prepare") {
  const jpeg = Buffer.from("/9j/4AAQSkZJRgABAQEAYABgAAD/2wBDAAMCAgMCAgMDAwMEAwMEBQgFBQQEBQoHBwYIDAoMDAsKCwsNDhIQDQ4RDgsLEBYQERMUFRUVDA8XGBYUGBIUFRT/2wBDAQMEBAUEBQkFBQkUDQsNFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBQUFBT/wAARCAB4AKADASIAAhEBAxEB/8QAHwAAAQUBAQEBAQEAAAAAAAAAAAECAwQFBgcICQoL/8QAtRAAAgEDAwIEAwUFBAQAAAF9AQIDAAQRBRIhMUEGE1FhByJxFDKBkaEII0KxwRVS0fAkM2JyggkKFhcYGRolJicoKSo0NTY3ODk6Q0RFRkdISUpTVFVWV1hZWmNkZWZnaGlqc3R1dnd4eXqDhIWGh4iJipKTlJWWl5iZmqKjpKWmp6ipqrKztLW2t7i5usLDxMXGx8jJytLT1NXW19jZ2uHi4+Tl5ufo6erx8vP09fb3+Pn6/8QAHwEAAwEBAQEBAQEBAQAAAAAAAAECAwQFBgcICQoL/8QAtREAAgECBAQDBAcFBAQAAQJ3AAECAxEEBSExBhJBUQdhcRMiMoEIFEKRobHBCSMzUvAVYnLRChYkNOEl8RcYGRomJygpKjU2Nzg5OkNERUZHSElKU1RVVldYWVpjZGVmZ2hpanN0dXZ3eHl6goOEhYaHiImKkpOUlZaXmJmaoqOkpaanqKmqsrO0tba3uLm6wsPExcbHyMnK0tPU1dbX2Nna4uPk5ebn6Onq8vP09fb3+Pn6/9oADAMBAAIRAxEAPwDq6KKK/os/KgooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooAKKKKACiiigAooooA//9k=", "base64");
  const paths = ["Shoot/Day1/Deep/photo.jpg", "Shoot/Day2/photo.jpg", "ShootExtra/photo.jpg", "Other/photo.jpg"];
  for (const [index, path] of paths.entries()) {
    const destination = resolve(fixture, path);
    await mkdir(resolve(destination, ".."), { recursive: true });
    await writeFile(destination, jpeg);
    await utimes(destination, 1700000000 + index * 120, 1700000000 + index * 120);
  }
  await unlink(resolve(fixture, "Shoot/Day1/photo.jpg")).catch(error => { if (error.code !== "ENOENT") throw error; });
  console.log(fixture);
  process.exit(0);
}
const port = process.env.CULLANT_CDP_PORT ?? "9222";
const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
const target = pages.find((page) => page.type === "page" && page.url.includes("localhost:1420"));
if (!target) throw new Error("Start npm run tauri:debug with the folder-ux-project fixture.");
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
let sequence = 0;
const pending = new Map();
socket.onmessage = ({ data }) => {
  const reply = JSON.parse(data);
  const task = pending.get(reply.id);
  if (!task) return;
  pending.delete(reply.id);
  if (reply.error) task.reject(new Error(reply.error.message));
  else task.resolve(reply.result);
};
function send(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    socket.send(JSON.stringify({ id, method, params }));
  });
}
async function evaluate(expression) {
  const result = await send("Runtime.evaluate", { expression: `(() => eval(${JSON.stringify(expression)}))()`, awaitPromise: true, returnByValue: true });
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  return result.result.value;
}
const settle = () => evaluate("new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)))");
async function waitFor(expression) {
  for (let attempt = 0; attempt < 50; attempt++) {
    if (await evaluate(expression)) return;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  throw new Error(`Timed out: ${expression}`);
}
async function key(key, code, windowsVirtualKeyCode, modifiers = 0) {
  await send("Input.dispatchKeyEvent", { type: "keyDown", key, code, windowsVirtualKeyCode, modifiers });
  await send("Input.dispatchKeyEvent", { type: "keyUp", key, code, windowsVirtualKeyCode, modifiers });
  await settle();
}
async function menuAction(label) {
  await evaluate(`(() => { const el = [...document.querySelectorAll('[role="menu"] button')].find(el => el.textContent.trim() === ${JSON.stringify(label)}); if (!el) throw new Error('Missing menu action'); el.click(); })()`);
  await settle();
}
async function folderMenu(name) {
  const bounds = await evaluate(`(() => { const el = [...document.querySelectorAll('.tree .node')].find(el => el.querySelector('.name')?.textContent === ${JSON.stringify(name)}); if (!el) throw new Error('Missing folder'); const r = el.getBoundingClientRect(); return {x:r.left+r.width/2,y:r.top+r.height/2}; })()`);
  await send("Input.dispatchMouseEvent", { type: "mousePressed", ...bounds, button: "right", clickCount: 1 });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", ...bounds, button: "right", clickCount: 1 });
  await settle();
}
async function drag(first, last, touch = false) {
  const bounds = await evaluate(`(() => { const rows = [...document.querySelectorAll('.draglist .handle')]; return [rows[${first}], rows[${last}]].map(el => { const r = el.getBoundingClientRect(); return { x: r.left + r.width / 2, y: r.top + r.height / 2 }; }); })()`);
  bounds[1].y += 6;
  if (touch) {
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [bounds[0]] });
    await send("Input.dispatchTouchEvent", { type: "touchMove", touchPoints: [bounds[1]] });
    await settle();
    await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  } else {
    await send("Input.dispatchMouseEvent", { type: "mousePressed", ...bounds[0], button: "left", clickCount: 1 });
    await send("Input.dispatchMouseEvent", { type: "mouseMoved", ...bounds[1], button: "left", buttons: 1 });
    await settle();
    await send("Input.dispatchMouseEvent", { type: "mouseReleased", ...bounds[1], button: "left", clickCount: 1 });
  }
  await settle();
}
async function click(selector) {
  await evaluate(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) throw new Error('Missing control: ' + ${JSON.stringify(selector)}); el.click(); })()`);
  await settle();
}
async function folderClick(path, modifiers = 0) {
  const bounds = await evaluate(`(() => {const el = document.querySelector(${JSON.stringify(path === null ? '.tree .root' : `.tree .node[title="${path}"]`)}); if (!el) throw new Error('Missing folder');const r=el.getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2};})()`);
  await send("Input.dispatchMouseEvent", { type: "mousePressed", ...bounds, button: "left", clickCount: 1, modifiers });
  await send("Input.dispatchMouseEvent", { type: "mouseReleased", ...bounds, button: "left", clickCount: 1, modifiers });
  await settle();
}
function assert(value, message) { if (!value) throw new Error(message); }
async function screenshot(name) {
  await mkdir(output, { recursive: true });
  const shot = await send('Page.captureScreenshot', { format: 'png' });
  await writeFile(`${output}/${name}.png`, Buffer.from(shot.data, 'base64'));
}
const results = [];
async function check(name, run) {
  try { await run(); results.push({ name, status: "PASS" }); }
  catch (error) { results.push({ name, status: "FAIL", error: error.message }); }
  console.log(`${results.at(-1).status}: ${name}${results.at(-1).error ? `: ${results.at(-1).error}` : ""}`);
}
await send("Page.reload");
for (let attempt = 0; attempt < 50; attempt++) {
  await new Promise(resolve => setTimeout(resolve, 100));
  if (await evaluate("!!document.querySelector('.tree')").catch(() => false)) break;
}
await evaluate(`(async () => {
  const loaded = path => import(performance.getEntriesByType('resource').find(entry => new URL(entry.name).pathname === path)?.name ?? path);
  const {session} = await loaded('/src/lib/stores/session.svelte.ts');
  const {catalog} = await loaded('/src/lib/stores/catalog.svelte.ts');
  const {view} = await loaded('/src/lib/stores/view.svelte.ts');
  const {settings} = await loaded('/src/lib/stores/settings.svelte.ts');
  const {api} = await loaded('/src/lib/api.ts');
  const {folders} = await loaded('/src/lib/stores/folders.svelte.ts').catch(() => ({}));
  window.ux = {session, catalog, view, settings, api, folders};
  const fixture = ${JSON.stringify(fixture)};
  const currentPath = catalog.project?.rootPath.replace(String.fromCharCode(92, 92, 63, 92), '');
  if (currentPath !== fixture && !currentPath?.startsWith(fixture + String.fromCharCode(92))) throw new Error('Use the disposable folder-ux-project fixture.');
  if (catalog.project.rootPath !== fixture) await catalog.open(fixture);
  await api.rescanProject();
  if (folders) { await api.setProjectSetting('ignoredFolders', '[]'); await folders.restore(); folders.collapsed.clear(); }
  session.clearFilters(); view.mode = 'grid'; session.folderTreeVisible = true;
})()`);
await settle();
await waitFor("!ux.catalog.scanning && !ux.catalog.preloading && ux.catalog.items.length === 4");
const original = await evaluate("({ barOrder: [...ux.settings.bottomBarOrder], barHidden: [...ux.settings.bottomBarHidden], radial: ux.settings.radialSlots.map(slot => ({...slot})), showFilmstrip: ux.session.showFilmstrip, rememberSession: ux.settings.rememberSession })");
await check("All leaves visible folder rows without scope highlights", async () => {
  assert(await evaluate("!document.querySelector('.tree .node.included, .tree .node.excluded, .tree .node:not(.root).active')"), "All highlights every folder");
});
await check("Ctrl adds and removes independent folders from the visible content", async () => {
  try {
    await folderClick('Shoot'); await folderClick('Other', 2);
    assert(await evaluate("ux.session.filtered.length === 3 && ux.session.filtered.some(item=>item.relPath.startsWith('Shoot/')) && ux.session.filtered.some(item=>item.relPath.startsWith('Other/')) && !ux.session.hasActiveFilters"), "Ctrl replaces the folder instead of adding it");
    await folderClick('Other', 2);
    assert(await evaluate("ux.session.filtered.length === 2 && ux.session.filtered.every(item=>item.relPath.startsWith('Shoot/'))"), "Ctrl does not deselect the folder");
  } finally { await evaluate("ux.session.clearFilters()"); await settle(); }
});
await check("Ctrl can hide and restore a subtree while viewing All", async () => {
  try {
    await folderClick(null); await folderClick('Shoot', 2);
    assert(await evaluate("ux.session.filtered.length === 2 && !ux.session.filtered.some(item=>item.relPath.startsWith('Shoot/'))"), "Ctrl does not deselect Shoot from All");
    await folderClick('Shoot', 2);
    assert(await evaluate("ux.session.filtered.length === 4 && document.querySelector('.tree .root').classList.contains('active') && !document.querySelector('.tree .node.included')"), "Ctrl does not restore All");
  } finally { await evaluate("ux.session.clearFilters()"); await settle(); }
});
await check("Ctrl can exclude and restore a child of a selected parent", async () => {
  try {
    await folderClick('Shoot'); await folderClick('Shoot/Day1', 2);
    assert(await evaluate("ux.session.filtered.length === 1 && ux.session.filtered[0].relPath.startsWith('Shoot/Day2/')"), "A selected parent prevents deselecting its child");
    await folderClick('Shoot/Day1', 2);
    assert(await evaluate("ux.session.filtered.length === 2 && ux.session.filtered.every(item=>item.relPath.startsWith('Shoot/'))"), "Ctrl does not restore the child subtree");
  } finally { await evaluate("ux.session.clearFilters()"); await settle(); }
});
await check("Shift selects a range in the visible folder tree", async () => {
  try {
    await folderClick('Other'); await folderClick('ShootExtra', 8);
    assert(await evaluate("ux.session.filtered.length === 2 && ux.session.filtered.every(item=>item.relPath.startsWith('Other/') || item.relPath.startsWith('ShootExtra/'))"), "Shift does not select the folder range");
    await folderClick('Shoot', 10);
    assert(await evaluate("ux.session.filtered.length === 4"), "Ctrl+Shift does not add the range");
  } finally { await evaluate("ux.session.clearFilters()"); await settle(); }
});
await check("Folder scope survives remount and reopening when session memory is on", async () => {
  try {
    await evaluate("ux.settings.setRememberSession(true)");
    await folderClick('Shoot'); await folderClick('Shoot/Day1', 2); await folderClick('Other', 2);
    await evaluate("ux.session.focusedIndex=0; ux.view.mode='viewer'"); await settle();
    await evaluate("ux.view.mode='grid'"); await settle();
    assert(await evaluate("ux.session.filtered.length===2 && !ux.session.filtered.some(item=>item.relPath.startsWith('Shoot/Day1/')) && ux.session.filtered.some(item=>item.relPath.startsWith('Other/'))"), "Remount loses folder selection");
    await evaluate("ux.catalog.open(ux.catalog.project.rootPath)"); await waitFor("!ux.session.restoring && !ux.catalog.preloading");
    assert(await evaluate("ux.session.filtered.length===2 && ux.session.filtered.some(item=>item.relPath.startsWith('Shoot/Day2/')) && ux.session.filtered.some(item=>item.relPath.startsWith('Other/'))"), "Reopening loses folder selection");
  } finally { await evaluate(`ux.settings.setRememberSession(${original.rememberSession}); ux.view.mode='grid'; ux.session.clearFilters()`); await settle(); }
});
await check("Removing the last selected folder shows none until All is selected", async () => {
  try {
    await folderClick('Other'); await folderClick('Other', 2);
    assert(await evaluate("ux.session.filtered.length===0 && !document.querySelector('.tree .root.active')"), "Deselecting the last folder selects it again");
    await folderClick(null);
    assert(await evaluate("ux.session.filtered.length===4"), "All does not restore an empty folder selection");
  } finally { await evaluate("ux.session.clearFilters()"); await settle(); }
});
await check("Range selection skips collapsed rows and preserves ignored folders", async () => {
  try {
    await evaluate("ux.folders.setIgnored('Other',true)"); await waitFor("!ux.folders.saving");
    await click('.tree .node[title="Shoot"] + .chevron');
    await folderClick('Shoot'); await folderClick('ShootExtra', 8);
    assert(await evaluate("ux.session.filtered.length===3 && !ux.session.filtered.some(item=>item.relPath.startsWith('Other/')) && ux.folders.ignored.includes('Other')"), "Range selection changes persistent exclusions");
    await screenshot('folder-range');
  } finally {
    await evaluate("ux.folders.collapsed.clear(); ux.session.clearFilters(); ux.folders.setIgnored('Other',false)"); await waitFor("!ux.folders.saving"); await settle();
  }
});
await check("Compare marks the active zoom pane without a permanent text badge", async () => {
  try {
    await evaluate("ux.session.focusedIndex = 0; ux.view.mode = 'compare'"); await settle();
    assert(await evaluate("document.querySelectorAll('.compare .pane').length === 2"), "Compare did not open");
    assert(await evaluate("!document.querySelector('.zoom-target')"), "Compare still shows Keyboard zoom");
    assert(await evaluate("document.querySelectorAll('.pane.zoom-active').length === 1"), "No active zoom pane");
    await evaluate("document.querySelectorAll('.compare .pane')[1].dispatchEvent(new PointerEvent('pointerdown', {bubbles:true,pointerType:'mouse',button:0}))"); await settle();
    assert(await evaluate("document.querySelectorAll('.compare .pane')[1].classList.contains('zoom-active') && document.querySelectorAll('.pane.zoom-active').length === 1"), "Clicking the right pane does not select keyboard zoom");
    await screenshot('compare');
  } finally { await evaluate("ux.view.mode = 'grid'"); await settle(); }
});
for (const scenario of ['wheel', 'double tap', 'keyboard', 'wheel with different source dimensions', 'double tap with different source dimensions', 'keyboard with different source dimensions']) {
  const gesture = scenario.split(' with ')[0];
  await check(`The first ${scenario} zoom grows from the fitted image size`, async () => {
    let originalDimensions;
    try {
      await evaluate("ux.session.clearFilters(); ux.session.focusedIndex = 0; ux.view.resetZoom(); ux.view.openedFromGridAt = 0");
      if (scenario.includes('different source')) {
        originalDimensions = await evaluate("(() => {const item=ux.session.focused;const original={id:item.id,width:item.width,height:item.height};item.width=320;item.height=240;return original;})()");
      }
      await evaluate("ux.view.mode='viewer'"); await settle();
      await waitFor("(() => {const image=document.querySelector('.viewer .frame img.fit');return image?.complete && image.naturalWidth>0 && !image.classList.contains('soft');})()");
      const before = await evaluate("(() => {const f=document.querySelector('.viewer .frame'),i=f.querySelector('img.fit'),r=f.getBoundingClientRect();const s=Math.min(r.width/i.naturalWidth,r.height/i.naturalHeight);return {width:i.naturalWidth*s,height:i.naturalHeight*s,x:r.left+r.width/2,y:r.top+r.height/2,naturalWidth:i.naturalWidth,naturalHeight:i.naturalHeight};})()");
      if (gesture === 'wheel') {
        await send('Input.dispatchMouseEvent', {type:'mouseWheel',x:before.x,y:before.y,deltaX:0,deltaY:-80,modifiers:2});
      } else if (gesture === 'keyboard') {
        await key('=', 'Equal', 187, 2);
      } else {
        await send('Emulation.setTouchEmulationEnabled', {enabled:true});
        for (let tap=0;tap<2;tap++) {
          await send('Input.dispatchTouchEvent', {type:'touchStart',touchPoints:[{x:before.x,y:before.y}]});
          await send('Input.dispatchTouchEvent', {type:'touchEnd',touchPoints:[]});
        }
        await send('Emulation.setTouchEmulationEnabled', {enabled:false});
      }
      await settle();
      await waitFor("!!document.querySelector('.viewer .frame.zoomed img.full')");
      const after = await evaluate("new Promise(resolve => {const widths=[];const start=performance.now();function sample(){const image=document.querySelector('.viewer .frame img.full');if(image)widths.push(image.getBoundingClientRect().width);if(performance.now()-start<250)requestAnimationFrame(sample);else resolve({width:widths.at(-1),minimum:Math.min(...widths),scale:ux.view.scale});}sample();})");
      assert(after.width > before.width * 1.05 && after.minimum >= before.width - 1, `First zoom shrinks the photo: ${JSON.stringify({before,after})}`);
      await screenshot(`first-zoom-${scenario.replaceAll(' ','-')}`);
      if (gesture === 'wheel') await send('Input.dispatchMouseEvent', {type:'mouseWheel',x:before.x,y:before.y,deltaX:0,deltaY:80,modifiers:2});
      else if (gesture === 'keyboard') await key('-', 'Minus', 189, 2);
      else await key('z', 'KeyZ', 90);
      await waitFor("!!document.querySelector('.viewer .frame img.fit')");
      const restored = await evaluate("(() => {const f=document.querySelector('.viewer .frame'),i=f.querySelector('img.fit');return i.naturalWidth*Math.min(f.clientWidth/i.naturalWidth,f.clientHeight/i.naturalHeight);})()");
      assert(Math.abs(restored-before.width)<1, "Zooming out does not return to the same fit size");
    } finally {
      await send('Emulation.setTouchEmulationEnabled', {enabled:false});
      await evaluate("ux.view.resetZoom(); ux.view.mode = 'grid'"); await settle();
      if (originalDimensions) await evaluate(`Object.assign(ux.catalog.items.find(item=>item.id===${originalDimensions.id}),${JSON.stringify(originalDimensions)})`);
    }
  });
}
await check("Compare keyboard zoom grows only the active pane when zoom sync is off", async () => {
  const synced = await evaluate("ux.settings.compareZoomSync");
  try {
    await evaluate("ux.settings.setCompareZoomSync(false); ux.session.focusedIndex=0; ux.view.mode='compare'"); await settle();
    await waitFor("[...document.querySelectorAll('.compare .frame img.fit')].length===2 && [...document.querySelectorAll('.compare .frame img.fit')].every(i=>i.complete && i.naturalWidth>0 && !i.classList.contains('soft'))");
    const before = await evaluate("(() => {const p=document.querySelectorAll('.compare .pane')[1],f=p.querySelector('.frame'),i=f.querySelector('img.fit');p.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'mouse',button:0}));return i.naturalWidth*Math.min(f.clientWidth/i.naturalWidth,f.clientHeight/i.naturalHeight);})()"); await settle();
    await key('=', 'Equal', 187, 2);
    await waitFor("!!document.querySelectorAll('.compare .pane')[1].querySelector('img.full')");
    assert(await evaluate(`!document.querySelectorAll('.compare .pane')[0].querySelector('.frame.zoomed') && document.querySelectorAll('.compare .pane')[1].querySelector('img.full').getBoundingClientRect().width > ${before} * 1.05`), "Compare zoom shrinks the active image or also zooms the other pane");
  } finally { await evaluate(`ux.settings.setCompareZoomSync(${synced}); ux.view.mode='grid'; ux.view.resetZoom()`); await settle(); }
});
await check("Collapsed branches survive a viewer round trip", async () => {
  await click('.tree .chevron');
  const before = await evaluate("document.querySelector('.tree .chevron').getAttribute('aria-expanded')");
  assert(before === "false", "Branch did not collapse");
  await evaluate("ux.session.focusedIndex = 0; ux.view.mode = 'viewer'"); await settle();
  await evaluate("ux.view.mode = 'grid'"); await settle();
  assert(await evaluate("document.querySelector('.tree .chevron').getAttribute('aria-expanded') === 'false'"), "Returning to grid expands the branch");
  await click('.tree .chevron');
});
await check("Nested branch collapse survives parent collapse and remount", async () => {
  await click('.tree .node[title="Shoot/Day1"] + .chevron');
  await click('.tree .node[title="Shoot"] + .chevron');
  await evaluate("ux.session.focusedIndex = 0; ux.view.mode = 'viewer'"); await settle();
  await evaluate("ux.view.mode = 'grid'"); await settle();
  await click('.tree .node[title="Shoot"] + .chevron');
  assert(await evaluate("document.querySelector('.tree .node[title=\"Shoot/Day1\"] + .chevron').getAttribute('aria-expanded') === 'false'"), "Nested collapse was lost");
  await click('.tree .node[title="Shoot/Day1"] + .chevron');
});
await check("Missing preview keeps the visible filmstrip and a grid button", async () => {
  await evaluate("ux.session.setShowFilmstrip(true); ux.view.mode = 'viewer'; ux.session.clearFocus()"); await settle();
  assert(await evaluate("!!document.querySelector('.viewer .filmstrip')"), "Filmstrip disappears when no photo is selected");
  await click('.viewer .empty button');
  assert(await evaluate("ux.view.mode === 'grid'"), "Grid button does not return to grid");
});
await check("A rescan after the preview file disappears keeps navigation available", async () => {
  const source = resolve(fixture, "Shoot/Day1/Deep/photo.jpg");
  const temporary = resolve(".playwright-mcp", `removed-preview-${process.pid}.jpg`);
  await evaluate("ux.session.folderFilter = 'Shoot/Day1'; ux.session.focusedIndex = 0; ux.session.setShowFilmstrip(true); ux.view.mode = 'viewer'"); await settle();
  await rename(source, temporary);
  try {
    await evaluate("ux.api.rescanProject()");
    await waitFor("ux.catalog.items.length === 3 && !!document.querySelector('.viewer .empty')");
    assert(await evaluate("!!document.querySelector('.viewer .filmstrip') && ux.session.showFilmstrip"), "Rescan hides the visible filmstrip");
    await click('.viewer .empty button');
    assert(await evaluate("ux.view.mode === 'grid'"), "Cannot return to grid after rescan");
  } finally {
    await rename(temporary, source);
    await evaluate("ux.api.rescanProject()");
    await waitFor("ux.catalog.items.length === 4");
    await evaluate("ux.session.clearFilters(); ux.view.mode = 'grid'"); await settle();
  }
});
await evaluate("ux.view.mode = 'grid'"); await settle();
await click('button[title="Settings"]');
await evaluate("ux.view.settingsPanel = 'touchBar'"); await settle();
await check("Move / Copy is configurable in the touch action bar", async () => {
  assert(await evaluate("[...document.querySelectorAll('.draglist .content')].some(el => el.textContent.includes('Move / Copy'))"), "Move / Copy is missing from settings");
});
await check("Each touch action bar setting has an icon beside its label", async () => {
  assert(await evaluate("[...document.querySelectorAll('.draglist .content')].length === 6 && [...document.querySelectorAll('.draglist .content')].every(el => el.querySelector('label svg'))"), "Touch action labels are missing icons");
  await screenshot('touch-bar');
});
await check("Touch action bar uses handles without arrow buttons", async () => {
  assert(await evaluate("document.querySelectorAll('.draglist .step').length === 0"), "Arrow buttons are still present");
  assert(await evaluate("document.querySelectorAll('.draglist .handle').length > 0"), "No drag handles");
});
await check("Mouse drag keeps capture through several touch bar row positions", async () => {
  const first = await evaluate("ux.settings.bottomBarOrder[0]");
  await drag(0, 3);
  assert(await evaluate(`ux.settings.bottomBarOrder[3] === ${JSON.stringify(first)}`), "Mouse drag stopped before the target row");
  assert(await evaluate("!document.querySelector('.draglist .dragging')"), "Drag remained active after release");
});
await check("Touch drag reorders the bar and releases its pointer", async () => {
  const first = await evaluate("ux.settings.bottomBarOrder[0]");
  await send("Emulation.setTouchEmulationEnabled", { enabled: true });
  await drag(0, 2, true);
  await send("Emulation.setTouchEmulationEnabled", { enabled: false });
  assert(await evaluate(`ux.settings.bottomBarOrder[2] === ${JSON.stringify(first)}`), "Touch drag stopped before the target row");
  assert(await evaluate("!document.querySelector('.draglist .dragging')"), "Touch drag remained active after release");
});
await check("The handle supports keyboard reorder and keeps focus", async () => {
  const first = await evaluate("ux.settings.bottomBarOrder[0]");
  await evaluate("document.querySelector('.draglist .handle').focus()");
  await key("ArrowDown", "ArrowDown", 40);
  assert(await evaluate(`ux.settings.bottomBarOrder[1] === ${JSON.stringify(first)} && document.activeElement === document.querySelectorAll('.draglist .handle')[1]`), "Keyboard move lost order or focus");
});
await evaluate("ux.view.settingsPanel = 'radial'"); await settle();
await check("Radial menu uses handles without arrow buttons", async () => {
  assert(await evaluate("document.querySelectorAll('.draglist .step').length === 0"), "Arrow buttons are still present");
});
await check("Mouse drag reorders radial sectors", async () => {
  const first = await evaluate("JSON.stringify(ux.settings.radialSlots[0])");
  await drag(0, 3);
  assert(await evaluate(`JSON.stringify(ux.settings.radialSlots[3]) === ${JSON.stringify(first)}`), "Radial sector did not move");
});
await check("Settings fit a phone viewport and touch drag still works", async () => {
  try {
    await send("Emulation.setDeviceMetricsOverride", { width: 390, height: 844, deviceScaleFactor: 1, mobile: true });
    await send("Emulation.setTouchEmulationEnabled", { enabled: true });
    await settle();
    const layout = await evaluate("({ viewport: innerWidth, editor: document.querySelector('.radial-editor').getBoundingClientRect().width, scroll: document.querySelector('.draglist').scrollWidth, list: document.querySelector('.draglist').clientWidth })");
    assert(layout.editor <= 390 && layout.scroll <= layout.list, `Settings overflow the phone viewport: ${JSON.stringify(layout)}`);
    const first = await evaluate("JSON.stringify(ux.settings.radialSlots[0])");
    await drag(0, 2, true);
    assert(await evaluate(`JSON.stringify(ux.settings.radialSlots[2]) === ${JSON.stringify(first)}`), "Radial touch drag fails at phone width");
    await evaluate("ux.view.settingsPanel = 'touchBar'"); await settle();
    assert(await evaluate("document.querySelector('.draglist').scrollWidth <= document.querySelector('.draglist').clientWidth"), "Touch bar settings overflow the phone viewport");
    const barFirst = await evaluate("ux.settings.bottomBarOrder[0]");
    await drag(0, 2, true);
    assert(await evaluate(`ux.settings.bottomBarOrder[2] === ${JSON.stringify(barFirst)}`), "Bar touch drag fails at phone width");
  } finally {
    await send("Emulation.clearDeviceMetricsOverride");
    await send("Emulation.setTouchEmulationEnabled", { enabled: false });
    await settle();
  }
});
for (let attempt = 0; attempt < 4 && await evaluate("!!document.querySelector('[role=dialog][aria-label=Settings]')"); attempt++) {
  await key("Escape", "Escape", 27);
}
assert(await evaluate("!document.querySelector('[role=dialog][aria-label=Settings]')"), "Settings did not close before folder checks");
await check("Folder project paths reject traversal and point to the selected directory", async () => {
  const path = await evaluate("ux.api.folderProjectPath('Shoot/Day1')");
  assert(path.replaceAll('\\', '/').endsWith('/Shoot/Day1'), "Wrong project directory");
  assert(await evaluate("ux.api.folderProjectPath('../Other').then(() => false, () => true)"), "Path traversal was accepted");
  assert(await evaluate("ux.api.folderProjectPath('Other/photo.jpg').then(() => false, () => true)"), "A file was accepted as a project directory");
});
await check("Folder selection does not activate Sort & Filter", async () => {
  await evaluate("ux.session.clearFilters()"); await settle();
  await click('.tree .node[title="Shoot"]');
  assert(await evaluate("ux.session.folderFilter === 'Shoot' && !ux.session.hasActiveFilters && document.querySelector('.filters-anchor > button').getAttribute('aria-pressed') === 'false'"), "Folder selection activates Sort & Filter");
  await evaluate("ux.session.minRating = 2"); await settle();
  assert(await evaluate("ux.session.hasActiveFilters"), "Rating does not activate Sort & Filter");
  await click('.tree .root');
  assert(await evaluate("ux.session.folderFilter === null && ux.session.minRating === 2"), "All changes other filters");
  await evaluate("ux.session.minRating = 0");
});
await check("Folder scope distinguishes selected, included, and excluded rows", async () => {
  await evaluate("ux.session.folderFilter = 'Shoot'"); await settle();
  assert(await evaluate("!!document.querySelector('.tree .node.included') && !!document.querySelector('.tree .node.excluded')"), "Descendants and hidden folders have no distinct styles");
});
await check("The folder panel has no redundant reset button", async () => {
  assert(await evaluate("ux.session.folderFilter === 'Shoot' && !document.querySelector('.tree .header button')"), "The redundant reset button is present");
});
await check("A folder context menu marks its source without changing selection", async () => {
  try {
    await folderMenu("Other");
    assert(await evaluate("document.querySelectorAll('.tree .node.menu-target').length === 1 && document.querySelector('.tree .node.menu-target').title === 'Other' && ux.session.folderFilter === 'Shoot'"), "The menu source is not marked");
    assert(await evaluate("!!document.activeElement?.closest('[role=menu]')"), `The menu has no keyboard focus: ${await evaluate("JSON.stringify({active:document.activeElement?.outerHTML.slice(0,180),dialogs:[...document.querySelectorAll('[role=dialog]')].map(el=>el.getAttribute('aria-label')),settingsOpen:ux.view.settingsOpen,settingsPanel:ux.view.settingsPanel,infoTip:ux.view.infoTip})")}`);
    await key("Escape", "Escape", 27);
    await waitFor("!document.querySelector('.tree .node.menu-target')");
    await folderMenu("Day1");
    assert(await evaluate("document.querySelector('.tree .node.menu-target')?.title === 'Shoot/Day1'"), "A nested folder menu has no source marker");
    await screenshot('folder-menu');
  } finally { await key("Escape", "Escape", 27); }
});
await check("Folder context menu can ignore and create a project", async () => {
  await evaluate("ux.session.clearFilters()"); await settle();
  await folderMenu("Shoot");
  assert(await evaluate("document.body.textContent.includes('Ignore folder')"), "No ignore action");
  assert(await evaluate("document.body.textContent.includes('Create a project for this folder')"), "No create project action");
});
await check("Ignoring a parent hides descendants and keeps similar folder names", async () => {
  await menuAction("Ignore folder");
  await waitFor("!ux.folders.saving");
  assert(await evaluate("ux.session.filtered.length === 2 && ux.session.filtered.some(item => item.relPath.startsWith('ShootExtra/')) && !ux.session.filtered.some(item => item.relPath.startsWith('Shoot/'))"), "Ignoring Shoot hides the wrong folders");
  assert(await evaluate("document.querySelector('.tree .root .count').textContent === '2'"), "All count includes ignored files");
});
await check("Reset to All preserves ignored folders", async () => {
  await evaluate("ux.session.folderFilter = 'Other'"); await settle();
  await click('.tree .root');
  assert(await evaluate("ux.session.folderFilter === null && ux.session.filtered.length === 2 && ux.folders.ignored.includes('Shoot')"), "All reset removed an exclusion");
});
await check("Ignored folders survive reopening with remember-session off", async () => {
  await evaluate("ux.settings.setRememberSession(false); ux.catalog.open(ux.catalog.project.rootPath)");
  await waitFor("!ux.session.restoring && !ux.catalog.preloading && ux.folders.ignored.includes('Shoot')");
  assert(await evaluate("ux.session.filtered.length === 2"), "Reopening restores ignored files");
});
await check("Ignored descendants explain which parent blocks them", async () => {
  await folderMenu("Day1");
  assert(await evaluate("[...document.querySelectorAll('[role=menu] button')].some(el => el.disabled && el.textContent.includes('Ignored by Shoot'))"), "Ignored child has no parent explanation");
  await key("Escape", "Escape", 27);
});
await check("A long press opens the folder menu without changing folder scope", async () => {
  await evaluate("ux.session.folderFilter = 'Other'"); await settle();
  const bounds = await evaluate("(() => { const el = [...document.querySelectorAll('.tree .node')].find(el => el.querySelector('.name')?.textContent === 'Shoot'); const r = el.getBoundingClientRect(); return {x:r.left+r.width/2,y:r.top+r.height/2}; })()");
  await send("Emulation.setTouchEmulationEnabled", { enabled: true });
  await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [bounds] });
  await waitFor("!!document.querySelector('[role=menu]')");
  await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
  await send("Emulation.setTouchEmulationEnabled", { enabled: false });
  assert(await evaluate("ux.session.folderFilter === 'Other'"), "Long press selects the held folder");
  assert(await evaluate("document.querySelector('.tree .node.menu-target .name')?.textContent === 'Shoot'"), "Long press does not mark its source");
  await key("Escape", "Escape", 27);
});
await check("Ignored folders can be restored from the tree", async () => {
  await folderMenu("Shoot");
  await menuAction("Show folder again");
  await waitFor("!ux.folders.saving");
  await evaluate("ux.session.folderFilter = null"); await settle();
  assert(await evaluate("ux.session.filtered.length === 4 && ux.folders.ignored.length === 0"), "Showing a folder does not restore its files");
});
await check("Other filters alone do not show the folder reset", async () => {
  await evaluate("ux.session.minRating = 2"); await settle();
  assert(await evaluate("!document.querySelector('.tree .header button')"), "Other filters show the folder reset");
  await evaluate("ux.session.minRating = 0");
});
await check("All offers no restore menu when there are no ignored folders", async () => {
  await folderMenu("All");
  assert(await evaluate("!document.querySelector('[role=menu]')"), "All opens a restore menu with no ignored folders");
});
await check("All restores ignored parents and children and saves the result", async () => {
  await evaluate("ux.folders.setIgnored('Shoot', true)"); await waitFor("!ux.folders.saving");
  await evaluate("ux.folders.setIgnored('Shoot/Day2', true)"); await waitFor("!ux.folders.saving");
  await evaluate("ux.session.folderFilter = 'Other'; ux.session.minRating = 2"); await settle();
  await folderMenu("All");
  assert(await evaluate("!!document.querySelector('.tree .root.menu-target')"), "All has no context marker");
  await screenshot('all-menu');
  await menuAction("Show all folders again");
  await waitFor("!ux.folders.saving");
  assert(await evaluate("ux.session.folderFilter === null && ux.folders.ignored.length === 0 && ux.session.minRating === 2"), "All does not restore ignored folders or changes other filters");
  await evaluate("ux.session.minRating = 0"); await settle();
  assert(await evaluate("ux.session.filtered.length === 4"), "Restored folders do not show their files");
  await evaluate("ux.catalog.open(ux.catalog.project.rootPath)");
  await waitFor("!ux.session.restoring && !ux.catalog.preloading");
  assert(await evaluate("ux.folders.ignored.length === 0 && ux.session.filtered.length === 4"), "Restored folders do not survive reopening");
});
await check("Holding All opens restore without selecting All until the action runs", async () => {
  await evaluate("ux.folders.setIgnored('Shoot', true)"); await waitFor("!ux.folders.saving");
  await evaluate("ux.session.folderFilter = 'Other'"); await settle();
  const bounds = await evaluate("(() => {const r=document.querySelector('.tree .root').getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2};})()");
  let touching = false;
  try {
    await send("Emulation.setTouchEmulationEnabled", { enabled: true });
    await send("Input.dispatchTouchEvent", { type: "touchStart", touchPoints: [bounds] });
    touching = true;
    await waitFor("!!document.querySelector('[role=menu]')");
    await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] }); touching = false; await settle();
    assert(await evaluate("ux.session.folderFilter === 'Other' && !!document.querySelector('.tree .root.menu-target')"), "Holding All changes selection");
    await menuAction("Show all folders again"); await waitFor("!ux.folders.saving");
    assert(await evaluate("ux.session.folderFilter === null && ux.folders.ignored.length === 0"), "Touch restore does not show all folders");
  } finally {
    if (touching) await send("Input.dispatchTouchEvent", { type: "touchEnd", touchPoints: [] });
    await send("Emulation.setTouchEmulationEnabled", { enabled: false });
    await key("Escape", "Escape", 27);
  }
});
await check("A failed restore preserves exclusions and the selected folder", async () => {
  await evaluate("ux.folders.setIgnored('Shoot', true)"); await waitFor("!ux.folders.saving");
  await evaluate("ux.session.folderFilter = 'Other'; ux.originalSetProjectSetting = ux.api.setProjectSetting; ux.api.setProjectSetting = () => Promise.reject('E2E save failure')"); await settle();
  try {
    await folderMenu("All"); await menuAction("Show all folders again"); await waitFor("!ux.folders.saving");
    assert(await evaluate("ux.folders.ignored.includes('Shoot') && ux.session.folderFilter === 'Other' && ux.catalog.error?.includes('E2E save failure')"), "Failed restore changes folder state");
  } finally {
    await evaluate("ux.api.setProjectSetting = ux.originalSetProjectSetting; ux.catalog.error = null");
    await key("Escape", "Escape", 27);
    await folderMenu("Shoot"); await menuAction("Show folder again"); await waitFor("!ux.folders.saving");
    await evaluate("ux.session.folderFilter = null"); await settle();
  }
});
await check("A folder project contains only that folder and has its own state", async () => {
  await folderMenu("Day1");
  await menuAction("Create a project for this folder");
  await waitFor("ux.catalog.project?.displayName === 'Day1' && !ux.catalog.preloading && ux.catalog.items.length === 1");
  assert(await evaluate("ux.catalog.items[0].relPath === 'Deep/photo.jpg' && ux.folders.ignored.length === 0"), "Child project has the wrong files or inherits parent exclusions");
  await evaluate(`ux.catalog.open(${JSON.stringify(fixture)})`);
  await waitFor("ux.catalog.project?.displayName === 'folder-ux-project' && !ux.catalog.preloading");
});
await check("A delayed folder action does not replace a newer project open", async () => {
  const path = await evaluate("ux.api.folderProjectPath('Shoot/Day1')");
  await evaluate("ux.originalFolderPath = ux.api.folderProjectPath; ux.api.folderProjectPath = () => new Promise(resolve => ux.resolveFolderPath = resolve)");
  try {
    await folderMenu("Day1");
    await menuAction("Create a project for this folder");
    await evaluate(`ux.catalog.open(${JSON.stringify(fixture)})`);
    const generation = await evaluate("ux.catalog.generation");
    await evaluate(`ux.resolveFolderPath(${JSON.stringify(path)})`);
    await new Promise(resolve => setTimeout(resolve, 150));
    assert(await evaluate(`ux.catalog.generation === ${generation} && ux.catalog.project.rootPath === ${JSON.stringify(fixture)}`), "A stale folder action replaces the newer project");
  } finally {
    await evaluate("ux.api.folderProjectPath = ux.originalFolderPath");
    await evaluate(`ux.catalog.open(${JSON.stringify(fixture)})`);
    await waitFor("!ux.session.restoring && !ux.catalog.preloading");
  }
});
await evaluate(`ux.settings.bottomBarOrder = ${JSON.stringify(original.barOrder)}; ux.settings.bottomBarHidden = ${JSON.stringify(original.barHidden)}; ux.settings.radialSlots = ${JSON.stringify(original.radial)}; ux.session.setShowFilmstrip(${original.showFilmstrip}); ux.settings.setRememberSession(${original.rememberSession}); localStorage.setItem('cullant.bottomBar', JSON.stringify({order:ux.settings.bottomBarOrder,hidden:ux.settings.bottomBarHidden})); localStorage.setItem('cullant.radial.slots', JSON.stringify(ux.settings.radialSlots.map(slot => slot.kind === 'more' ? 'more' : slot.kind === 'group' ? 'group:' + slot.id : 'cmd:' + slot.id))); ux.session.clearFilters(); ux.session.folderFilter = 'Shoot'`);
await settle();
await mkdir(output, { recursive: true });
await writeFile(`${output}/results.json`, JSON.stringify({ date: new Date().toISOString(), target: target.url, results }, null, 2));
const shot = await send('Page.captureScreenshot', { format: 'png' });
await writeFile(`${output}/screen.png`, Buffer.from(shot.data, 'base64'));
socket.close();
process.exitCode = results.some(result => result.status === 'FAIL') ? 1 : 0;
