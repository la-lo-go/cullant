import { mkdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";

const output=resolve(process.argv[2]??'.playwright-mcp/ui-polish');
const targets=await(await fetch(`http://127.0.0.1:${process.env.CULLANT_CDP_PORT??9222}/json/list`)).json();
const target=targets.find(page=>page.type==='page'&&page.url.includes('localhost:1420'));
if(!target) throw new Error('Start the debug app with the disposable preview-cache-project.');
const socket=new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
let sequence=0;
const pending=new Map(),runtimeErrors=[],results=[],observations=[];
socket.onmessage=({data})=>{
  const reply=JSON.parse(data),task=pending.get(reply.id);
  if(reply.method==='Runtime.exceptionThrown') runtimeErrors.push(reply.params);
  if(!task)return;
  pending.delete(reply.id);
  if(reply.error)task.reject(new Error(reply.error.message));else task.resolve(reply.result);
};
socket.onclose=()=>{for(const task of pending.values())task.reject(new Error('App connection closed'));pending.clear();};
function send(method,params={}){return new Promise((resolve,reject)=>{const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(expression){
  const reply=await send('Runtime.evaluate',{expression:`(async()=>{const json=JSON.stringify(await eval(${JSON.stringify(expression)}));return json===undefined?null:JSON.parse(json);})()`,awaitPromise:true,returnByValue:true});
  if(reply.exceptionDetails)throw new Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);
  return reply.result.value;
}
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
const settle=()=>evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
function assert(condition,message){if(!condition)throw new Error(message);}
async function waitFor(expression){for(let i=0;i<100;i++){if(await evaluate(expression).catch(()=>false))return;await pause(50);}throw new Error(`Timed out: ${expression}`);}
async function screenshot(name){const shot=await send('Page.captureScreenshot',{format:'png'});await writeFile(join(output,`${name}.png`),Buffer.from(shot.data,'base64'));}
async function check(name,run){try{await run();results.push({name,status:'PASS'});}catch(error){results.push({name,status:'FAIL',error:error.message});await screenshot(`failure-${results.length}`).catch(()=>{});}console.log(`${results.at(-1).status}: ${name}${results.at(-1).error?`: ${results.at(-1).error}`:''}`);}
async function box(selector){return evaluate(`(()=>{const el=document.querySelector(${JSON.stringify(selector)});if(!el)throw new Error('Missing '+${JSON.stringify(selector)});el.scrollIntoView({block:'nearest'});const r=el.getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2};})()`);}
async function mouseMove(selector){const point=selector?await box(selector):{x:5,y:5};await send('Input.dispatchMouseEvent',{type:'mouseMoved',...point});await settle();}
async function touch(selector){const point=await box(selector);await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[point]});await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await pause(100);await settle();}
async function key(key,code,windowsVirtualKeyCode,modifiers=0){await send('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode,modifiers});await send('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode,modifiers});await settle();}
async function folderMenu(path){await key('Escape','Escape',27);const point=await box(`.tree .node[title="${path}"]`);await send('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'right',clickCount:1});await send('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'right',clickCount:1});await settle();return evaluate("[...document.querySelectorAll('[role=menu] button')].map(button=>button.textContent.trim())");}
async function panel(id=null){await evaluate(`up.view.infoTip=null;up.session.filtersPanelOpen=false;if(!document.querySelector('[role=dialog][aria-label=Settings]'))document.querySelector('button[title=Settings]').click();up.view.settingsPanel=${JSON.stringify(id)}`);await settle();}
async function data(rows){await evaluate(`document.querySelector('.dialog .close-x')?.click();up.session.clearFilters();up.folders.selectOnly(null);up.catalog.items=${JSON.stringify(rows)};up.catalog.mediaCounts={photos:${rows.length},videos:0};up.view.mode='grid';up.session.filtersPanelOpen=true`);await settle();}
await mkdir(output,{recursive:true});
let original;
try{
  await send('Runtime.enable');await send('Page.reload');
  await waitFor("performance.getEntriesByType('resource').some(entry=>new URL(entry.name).pathname==='/src/lib/stores/catalog.svelte.ts')");await pause(200);
  await evaluate(`(async()=>{
    const loaded=path=>import(performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname===path)?.name??path);
    const {catalog}=await loaded('/src/lib/stores/catalog.svelte.ts');const {session}=await loaded('/src/lib/stores/session.svelte.ts');
    const {settings}=await loaded('/src/lib/stores/settings.svelte.ts');const {view}=await loaded('/src/lib/stores/view.svelte.ts');const {folders}=await loaded('/src/lib/stores/folders.svelte.ts');
    if(!catalog.project)await catalog.open(${JSON.stringify(resolve('.playwright-mcp/preview-cache-project'))});
    if(!catalog.project?.rootPath.replaceAll('\\\\','/').toLowerCase().endsWith('/.playwright-mcp/preview-cache-project'))throw new Error('Use only the disposable preview-cache-project.');
    window.up={catalog,session,settings,view,folders,items:JSON.parse(JSON.stringify(catalog.items)),storage:{...localStorage},scope:JSON.parse(JSON.stringify(folders.scope)),collapsed:[...folders.collapsed],tree:session.folderTreeVisible,todayDay:session.todayDay};
    settings.setRememberSession(false);session.clearFilters();folders.selectOnly(null);folders.collapsed.clear();session.folderTreeVisible=true;view.mode='grid';
  })()`);
  await waitFor('up.catalog.items.length>=8 && !up.catalog.ingesting');
  await evaluate('up.items=JSON.parse(JSON.stringify(up.catalog.items))');
  original=await evaluate('({remember:up.settings.rememberSession,rows:up.items.length})');
  const base=await evaluate('up.items.slice(0,8)');
  const today=new Date().toISOString().slice(0,10), midnight=Date.parse(`${today}T00:00:00Z`)/1000;
  const seconds=[midnight,midnight+86399,midnight-1,midnight-6*86400,midnight-7*86400+86399,midnight+86400,midnight-30*86400,midnight-31*86400];
  const dated=base.map((row,index)=>({...row,captureTime:seconds[index],isPrimary:true,groupSize:1,decoupled:false}));
  await data(dated);
  await check('Capture day follows Sort and starts its select with Today and Last 7 days',async()=>{
    const labels=await evaluate("[...document.querySelectorAll('[aria-label=\"Sort & filter\"] section>.lbl')].map(el=>el.textContent.trim())");
    const options=await evaluate("[...document.querySelector('[aria-label=\"Capture day\"]').options].map(option=>option.value)");
    observations.push({case:'date-order',labels,options});
    const sortIndex=labels.indexOf('Sort'),dateIndex=labels.indexOf('Capture day');
    assert(sortIndex>=0&&dateIndex===sortIndex+1&&labels[dateIndex+1]==='File name','Capture day is not between Sort and File name');
    assert(options[0]==='today'&&options[1]==='last7days'&&options[2]==='','Relative presets do not precede All capture days in the same select');
  });
  await check('Today and Last 7 days use UTC boundaries and exclude future dates',async()=>{
    await evaluate(`up.session.todayDay=${JSON.stringify(today)};up.session.dateFilter='today'`);await settle();
    const todayIds=await evaluate('up.session.filtered.map(row=>row.id)');
    assert(JSON.stringify(todayIds)===JSON.stringify(dated.slice(0,2).map(row=>row.id)),'Today does not match both UTC boundary photos');
    await evaluate("up.session.dateFilter='last7days'");await settle();
    const ids=await evaluate('up.session.filtered.map(row=>row.id)');
    assert(JSON.stringify(ids)===JSON.stringify(dated.slice(0,4).map(row=>row.id)),'Last 7 days includes an old or future photo, or excludes its first day');
    await evaluate("up.session.clearFilters()");assert(await evaluate('up.session.dateFilter===null && up.session.filtered.length===8'),'Clear leaves a date preset active');
  });
  await check('Single-day visibility and an active empty preset remain correct',async()=>{
    await data([dated[6]]);assert(!await evaluate("!!document.querySelector('[aria-label=\"Capture day\"]')"),'A single historical day has an unnecessary date filter');
    await data([dated[0]]);assert(await evaluate("!!document.querySelector('[aria-label=\"Capture day\"] option[value=today]')"),'A single current day hides Today');
    await data([dated[6]]);await evaluate("up.session.dateFilter='today'");await settle();
    assert(await evaluate("document.querySelector('[aria-label=\"Capture day\"]')?.value==='today' && up.session.filtered.length===0"),'An active preset disappears when it has no matching photos');
  });
  const nested=dated.slice(0,4).map((row,index)=>({...row,relPath:['A/B/C/one.png','A/B/two.png','A/D/three.png','Leaf/four.png'][index]}));
  await data(nested);await evaluate('up.session.filtersPanelOpen=false');await settle();
  await check('Folder branch actions offer only useful operations',async()=>{
    let labels=await folderMenu('Leaf');assert(!labels.includes('Expand branch')&&!labels.includes('Collapse branch'),'A leaf offers branch actions');
    labels=await folderMenu('A');assert(labels.includes('Collapse branch')&&!labels.includes('Expand branch'),'A fully expanded branch offers Expand');
    await key('Escape','Escape',27);await evaluate("up.folders.collapsed.add('A')");await settle();
    labels=await folderMenu('A');assert(labels.includes('Expand branch')&&!labels.includes('Collapse branch'),'A collapsed branch offers Collapse');
    await key('Escape','Escape',27);await evaluate("up.folders.collapsed.delete('A');up.folders.collapsed.add('A/B')");await settle();
    labels=await folderMenu('A');assert(labels.includes('Expand branch')&&labels.includes('Collapse branch'),'A partly expanded branch does not offer both useful actions');
    await screenshot('folder-branch');await key('Escape','Escape',27);
  });
  await check('Mouse hover uses the theme fill and keeps focus borders separate',async()=>{
    await mouseMove(null);const before=await evaluate("(()=>{const s=getComputedStyle(document.querySelector('.tree .node[title=Leaf]'));return{border:s.borderColor,outline:s.outlineStyle,shadow:s.boxShadow};})()");
    await mouseMove('.tree .node[title=Leaf]');const after=await evaluate("(()=>{const s=getComputedStyle(document.querySelector('.tree .node[title=Leaf]')),probe=document.createElement('span');probe.style.backgroundColor=getComputedStyle(document.documentElement).getPropertyValue('--hover').trim();return{border:s.borderColor,outline:s.outlineStyle,shadow:s.boxShadow,fill:s.backgroundColor,themeFill:probe.style.backgroundColor};})()");
    observations.push({case:'folder-hover',before,after});
    assert(after.fill===after.themeFill,'Hover fill does not match the theme');
    assert(before.border===after.border&&before.outline===after.outline&&before.shadow===after.shadow,'Hover adds a focus border');
  });
  await panel();
  await check('Select hover leaves the border neutral',async()=>{
    await mouseMove(null);const before=await evaluate("getComputedStyle(document.querySelector('.dialog select[aria-label=Threshold]')).borderColor");
    await mouseMove('.dialog select[aria-label=Threshold]');const after=await evaluate("getComputedStyle(document.querySelector('.dialog select[aria-label=Threshold]')).borderColor");
    assert(before===after,'Select hover adds the accent focus border');
  });
  await panel('colorLabelNames');
  await check('Five rounded color swatches replace label text and text focus uses a cyan border',async()=>{
    const swatches=await evaluate("[...document.querySelectorAll('.label-name .label-swatch')].map(el=>{const s=getComputedStyle(el),r=el.getBoundingClientRect();return{color:s.backgroundColor,radius:s.borderRadius,width:r.width,height:r.height,text:el.textContent};})");
    assert(swatches.length===5,'Color name rows do not have five swatches');
    assert(swatches.every(s=>s.width===s.height&&s.width>0&&parseFloat(s.radius)>0&&!s.text.trim()),'Swatches are not empty rounded squares');
    assert(new Set(swatches.map(s=>s.color)).size===5,'Color swatches do not use five distinct colors');
    await evaluate("document.querySelector('.label-name input[aria-label=\"Red label name\"]').focus()");await key('ArrowRight','ArrowRight',39);
    const focus=await evaluate("(()=>{const el=document.querySelector('.label-name input[aria-label=\"Red label name\"]'),s=getComputedStyle(el),probe=document.createElement('span');probe.style.color=s.getPropertyValue('--accent');return{border:s.borderColor,accent:probe.style.color,outline:s.outlineStyle};})()");
    assert(focus.border===focus.accent&&focus.outline==='none','Typing focus does not use one cyan border');await screenshot('color-names');
  });
  await send('Emulation.setTouchEmulationEnabled',{enabled:true});await panel();
  await check('Touch select opens normally and cancellation keeps its focus appearance neutral',async()=>{
    await mouseMove(null);const selector='.dialog select[aria-label=Threshold]';await touch(selector);
    assert(await evaluate("document.activeElement===document.querySelector('.dialog select[aria-label=Threshold]')"),'Opening the native picker blurred it');
    await touch('.dialog .head h2');const style=await evaluate("(()=>{const el=document.querySelector('.dialog select[aria-label=Threshold]'),s=getComputedStyle(el),probe=document.createElement('span');probe.style.color=s.getPropertyValue('--accent');return{border:s.borderColor,accent:probe.style.color,outline:s.outlineStyle};})()");
    observations.push({case:'touch-select-cancel',style});assert(style.border!==style.accent&&style.outline==='none','Cancelled touch picker keeps an accent ring');
    await touch(selector);await touch('.dialog .head h2');await evaluate("(()=>{const el=document.querySelector('.dialog select[aria-label=Threshold]');el.value=el.value==='fixed'?'adaptive':'fixed';el.dispatchEvent(new Event('change',{bubbles:true}));})()");await settle();
    assert(await evaluate("document.activeElement!==document.querySelector('.dialog select[aria-label=Threshold]')"),'Changed touch selection keeps the select focused');
  });
  await panel();
  await check('Touch switches release focus and text entry keeps focus',async()=>{
    const checkbox='.dialog .row.whole-row:has(input[aria-label="Progressive loading"])';await mouseMove(null);const before=await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(checkbox)})).backgroundColor`);await touch(checkbox);
    assert(await evaluate("document.activeElement!==document.querySelector('.sw-input[aria-label=\"Progressive loading\"]')"),'A touch switch retains checkbox focus');
    assert(await evaluate(`getComputedStyle(document.querySelector(${JSON.stringify(checkbox)})).backgroundColor===${JSON.stringify(before)}`),'A touched switch retains sticky hover fill');
    await panel('colorLabelNames');await touch('.label-name input[aria-label="Red label name"]');await send('Input.insertText',{text:'E2E label'});
    assert(await evaluate("document.activeElement===document.querySelector('.label-name input[aria-label=\"Red label name\"]') && document.activeElement.value.includes('E2E label')"),'Touch text entry loses focus while typing');
  });
  await panel('touchBar');
  await check('Touch drag releases its handle and keyboard reorder keeps focus',async()=>{
    await evaluate("document.querySelector('.draglist .handle').focus()");const [first,last]=await evaluate("[document.querySelectorAll('.draglist .handle')[0],document.querySelectorAll('.draglist .handle')[2]].map(el=>{const r=el.getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2};})");
    await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[first]});await send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{...last,y:last.y+6}]});await pause(100);await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await settle();
    assert(await evaluate("!document.activeElement?.matches('.draglist .handle')"),'Touch drag leaves its handle focused');
    await send('Emulation.setTouchEmulationEnabled',{enabled:false});await evaluate("document.querySelector('.draglist .handle').focus()");await key('ArrowDown','ArrowDown',40);
    assert(await evaluate("document.activeElement?.matches('.draglist .handle')"),'Keyboard reorder loses handle focus');
  });
  await send('Emulation.setTouchEmulationEnabled',{enabled:false});await panel();
  await check('Keyboard checkbox activation preserves focus and its visible ring',async()=>{
    await evaluate("document.querySelector('.dialog .sw-input[aria-label=\"Progressive loading\"]').focus()");await key(' ','Space',32);
    assert(await evaluate("document.activeElement===document.querySelector('.dialog .sw-input[aria-label=\"Progressive loading\"]') && document.activeElement.matches(':focus-visible')"),'Keyboard checkbox activation loses its focus ring');
  });
  await check('Tab restores keyboard focus appearance after a touch interaction',async()=>{
    await send('Emulation.setTouchEmulationEnabled',{enabled:true});await touch('.dialog select[aria-label=Threshold]');await key('Escape','Escape',27);
    await key('Tab','Tab',9);await evaluate("document.querySelector('.dialog select[aria-label=Threshold]').focus()");
    const style=await evaluate("(()=>{const el=document.querySelector('.dialog select[aria-label=Threshold]'),s=getComputedStyle(el),probe=document.createElement('span');probe.style.color=s.getPropertyValue('--accent');return{focused:el.matches(':focus-visible'),border:s.borderColor,accent:probe.style.color};})()");
    assert(style.focused&&style.border===style.accent,'Tab does not restore the cyan keyboard focus border');
  });
  await screenshot('final');
}catch(error){results.push({name:'Harness setup',status:'FAIL',error:error.stack??error.message});console.error(error);}
finally{
  await send('Emulation.setTouchEmulationEnabled',{enabled:false}).catch(()=>{});
  await evaluate("(()=>{if(!window.up)return;document.querySelector('.dialog .close-x')?.click();up.view.settingsPanel=null;up.view.infoTip=null;up.session.filtersPanelOpen=false;up.session.clearFilters();up.catalog.items=up.items;up.catalog.mediaCounts={photos:up.items.length,videos:0};up.folders.scope=up.scope;up.folders.collapsed.clear();for(const path of up.collapsed)up.folders.collapsed.add(path);up.session.folderTreeVisible=up.tree;up.session.todayDay=up.todayDay;for(const key of Object.keys(localStorage))if(key.startsWith('cullant.'))localStorage.removeItem(key);for(const [key,value]of Object.entries(up.storage))localStorage.setItem(key,value);})()").catch(()=>{});
  await writeFile(join(output,'results.json'),JSON.stringify({date:new Date().toISOString(),target:target.url,results,observations,runtimeErrors,original},null,2));
  await send('Page.reload').catch(()=>{});socket.close();
}
process.exitCode=results.some(result=>result.status==='FAIL')?1:0;
