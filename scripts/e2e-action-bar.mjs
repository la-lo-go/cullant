import { mkdir, writeFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';

// Check lost saved items, wrong action targets, video zoom controls, wrapped rows,
// duplicate touch actions, persistent tooltips, lost active fills, and invalid defaults.
const output=resolve(process.argv[2]??'.playwright-mcp/action-bar');
const targets=await(await fetch(`http://127.0.0.1:${process.env.CULLANT_CDP_PORT??9222}/json/list`)).json();
const target=targets.find(page=>page.type==='page'&&page.url.includes('localhost:1420'));
if(!target)throw new Error('Start the debug app with the disposable preview-cache-project.');
const socket=new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
let sequence=0;
const pending=new Map(),results=[],observations=[],runtimeErrors=[];
socket.onmessage=({data})=>{
  const reply=JSON.parse(data),task=pending.get(reply.id);
  if(reply.method==='Runtime.exceptionThrown')runtimeErrors.push(reply.params);
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
async function click(selector){const point=await box(selector);await send('Input.dispatchMouseEvent',{type:'mousePressed',...point,button:'left',clickCount:1});await send('Input.dispatchMouseEvent',{type:'mouseReleased',...point,button:'left',clickCount:1});await settle();}
async function touch(selector,hold=50){const point=await box(selector);await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[point]});await pause(hold);await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await pause(100);await settle();}
async function key(key,code,windowsVirtualKeyCode){await send('Input.dispatchKeyEvent',{type:'keyDown',key,code,windowsVirtualKeyCode});await send('Input.dispatchKeyEvent',{type:'keyUp',key,code,windowsVirtualKeyCode});await settle();}
async function bootstrap(){
  await waitFor("performance.getEntriesByType('resource').some(entry=>new URL(entry.name).pathname==='/src/lib/stores/catalog.svelte.ts')");await pause(150);
  await evaluate(`(async()=>{
    const loaded=path=>import(performance.getEntriesByType('resource').find(entry=>new URL(entry.name).pathname===path)?.name??path);
    const {catalog}=await loaded('/src/lib/stores/catalog.svelte.ts');const {session}=await loaded('/src/lib/stores/session.svelte.ts');
    const {settings}=await loaded('/src/lib/stores/settings.svelte.ts');const {view}=await loaded('/src/lib/stores/view.svelte.ts');const {folders}=await loaded('/src/lib/stores/folders.svelte.ts');
    if(!catalog.project)await catalog.open(${JSON.stringify(resolve('.playwright-mcp/preview-cache-project'))});
    if(!catalog.project?.rootPath.replaceAll('\\\\','/').toLowerCase().endsWith('/.playwright-mcp/preview-cache-project'))throw new Error('Use only the disposable preview-cache-project.');
    window.ab={catalog,session,settings,view,folders,autoCalls:0,setFast:settings.setFastCulling};
    settings.setRememberSession(false);session.clearFilters();session.clearSelection();folders.selectOnly(null);settings.setCollapseBursts(false);settings.setFastCulling(false);session.autoAdvancePref=false;session.capsLockActive=false;view.mode='viewer';session.focusedIndex=0;
  })()`);
  await waitFor('ab.catalog.items.length>=8 && !ab.catalog.ingesting');
  await evaluate("ab.session.focusedIndex=0;ab.view.mode='viewer'");
  await waitFor('!!ab.session.focused');
  if(!await evaluate("document.querySelector('.touchbar')?.classList.contains('forced')"))await click('[aria-label="Show/hide the action bar"]');
  await settle();
}
async function settingsPanel(){await evaluate("ab.view.infoTip=null;if(!document.querySelector('[role=dialog][aria-label=Settings]'))document.querySelector('button[aria-label=Settings],button[title=Settings]').click()");await settle();await evaluate("ab.view.settingsPanel='touchBar'");await settle();}
async function closeSettings(){await evaluate("document.querySelector('.dialog .close-x')?.click();ab.view.settingsPanel=null");await settle();}
async function tooltipFor(selector){await mouseMove(null);await mouseMove(selector);await pause(700);return evaluate("document.querySelector('[role=tooltip]')?.textContent.trim()??''");}
const framingButtons='[role=dialog][aria-label=View] section:nth-of-type(2) .seg';
async function viewPanel(){if(!await evaluate("!!document.querySelector('[role=dialog][aria-label=View]')"))await click('.view-anchor>button');await settle();}
async function framingState(){await evaluate("ab.view.mode='grid'");await settle();await viewPanel();await waitFor("document.querySelectorAll('.cell .photo').length>0");return evaluate(`({setting:ab.settings.gridPhotoFit,pressed:[...document.querySelectorAll(${JSON.stringify(framingButtons)})].filter(el=>el.getAttribute('aria-pressed')==='true').map(el=>el.textContent.trim()),fitted:[...document.querySelectorAll('.cell .photo')].every(el=>el.classList.contains('fit'))})`);}
async function hoverState(selector){
  const read=()=>evaluate(`(()=>{const s=getComputedStyle(document.querySelector(${JSON.stringify(selector)})),probe=document.createElement('span');probe.style.backgroundColor=getComputedStyle(document.documentElement).getPropertyValue('--hover').trim();return{fill:s.backgroundColor,border:s.borderColor,outline:s.outlineStyle,shadow:s.boxShadow,theme:probe.style.backgroundColor};})()`);
  await mouseMove(null);await pause(180);const before=await read();await mouseMove(selector);await pause(180);const hover=await read();await mouseMove(null);await pause(180);return{selector,before,hover,after:await read()};
}
function assertHover(states){
  const badFill=states.filter(s=>s.hover.fill!==s.hover.theme).map(s=>s.selector);
  assert(badFill.length===0,`Hover fill does not match the theme: ${badFill.join(', ')}`);
  assert(states.every(s=>s.before.border===s.hover.border&&s.before.outline===s.hover.outline&&s.before.shadow===s.hover.shadow),'Hover changes a border, outline, or shadow');
  assert(states.every(s=>s.after.fill===s.before.fill),'Leaving a control does not restore its original fill');
}
async function fixtureRows(video=false){await evaluate("ab.view.mode='grid'");await settle();await evaluate(`ab.catalog.items=${JSON.stringify(original.items.slice(0,8).map(row=>({...row,kind:video?2:row.kind,isPrimary:true,groupSize:1,decoupled:false})))};ab.catalog.media=${JSON.stringify(video?'videos':'photos')};ab.catalog.mediaCounts={photos:${video?0:8},videos:${video?8:0}};ab.session.clearFilters();ab.session.clearSelection();ab.session.focusedIndex=0;ab.view.mode='compare'`);await settle();}
await mkdir(output,{recursive:true});
let original;
try{
  await send('Runtime.enable');await send('Page.reload');
  await waitFor("performance.getEntriesByType('resource').some(entry=>new URL(entry.name).pathname==='/src/lib/stores/catalog.svelte.ts')");
  const storage=await evaluate('({...localStorage})');
  await bootstrap();
  original={storage,items:await evaluate('ab.catalog.items'),media:await evaluate('ab.catalog.media'),counts:await evaluate('ab.catalog.mediaCounts')};
  await check('Bottom action buttons use the theme hover fill and restore their active state',async()=>{
    await evaluate('ab.settings.resetBottomBar();ab.settings.setFastCulling(true)');await settle();
    const buttons=await evaluate("[...document.querySelectorAll('.touchbar button')].filter(el=>!el.disabled&&el.getBoundingClientRect().width>0).map((el,index)=>{el.dataset.e2eHover=String(index);return'[data-e2e-hover=\"'+index+'\"]';})");
    const states=[];for(const selector of buttons)states.push(await hoverState(selector));
    observations.push({case:'bottom-hover',states});assert(await evaluate("document.querySelector('.touchbar [aria-label=Auto]').classList.contains('active')"),'Auto was not active for the hover check');assertHover(states);
  });
  await evaluate('ab.settings.setFastCulling(false)');
  await check('Active and inactive Grid, Loupe, and Compare controls use the theme hover fill',async()=>{
    const states=[];
    for(const [mode,label]of [['grid','Grid'],['viewer','Loupe'],['compare','Compare']]){
      await evaluate(`ab.view.mode=${JSON.stringify(mode)};ab.session.focusedIndex=0`);await settle();
      for(const targetLabel of ['Grid','Loupe','Compare'])states.push({...await hoverState(`.toolbar [aria-label=${targetLabel}]`),mode,active:targetLabel===label});
    }
    observations.push({case:'view-selector-hover',states});assertHover(states);
  });
  await evaluate("ab.view.mode='grid';ab.session.viewPanelOpen=false");await settle();
  await check('View panel framing and size controls use the theme hover fill when active or inactive',async()=>{
    await viewPanel();const states=[];
    for(const [section,count]of [[1,3],[2,2]]){
      for(let index=1;index<=count;index++){
        const selector=`[role=dialog][aria-label=View] section:nth-of-type(${section}) .seg:nth-child(${index})`;
        await click(selector);assert(await evaluate(`document.querySelector(${JSON.stringify(selector)}).classList.contains('active')`),'View control did not become active');
        states.push({...await hoverState(selector),active:true});
        const inactive=`[role=dialog][aria-label=View] section:nth-of-type(${section}) .seg:nth-child(${index===count?1:index+1})`;
        states.push({...await hoverState(inactive),active:false});
      }
    }
    observations.push({case:'view-panel-hover',states});assertHover(states);
  });
  await evaluate('ab.session.viewPanelOpen=false');await settle();
  await check('Missing and invalid thumbnail framing preferences default to Fit whole photo',async()=>{
    const states=[];
    for(const raw of [null,'"unexpected"','{invalid']){
      await evaluate(raw===null?"localStorage.removeItem('cullant.gridPhotoFit')":`localStorage.setItem('cullant.gridPhotoFit',${JSON.stringify(raw)})`);
      await send('Page.reload');await bootstrap();states.push({raw,...await framingState()});
    }
    observations.push({case:'framing-default',states});
    assert(states.every(s=>s.setting==='fit'&&s.pressed[0]==='Fit whole photo'&&s.fitted),'A missing or invalid preference defaults to Fill cell');
  });
  await check('Explicit Fill cell and Fit whole photo choices survive reload and update the grid',async()=>{
    const states=[];
    for(const [value,index,label]of [['fill',2,'Fill cell'],['fit',1,'Fit whole photo']]){
      await viewPanel();await click(`${framingButtons}:nth-child(${index})`);
      assert(await evaluate(`ab.settings.gridPhotoFit===${JSON.stringify(value)}&&JSON.parse(localStorage.getItem('cullant.gridPhotoFit'))===${JSON.stringify(value)}`),'The framing choice is not saved');
      await send('Page.reload');await bootstrap();const state=await framingState();states.push({expected:value,...state});
      assert(state.setting===value&&state.pressed[0]===label&&state.fitted===(value==='fit'),'The saved framing choice is lost or the grid does not reflect it');
    }
    observations.push({case:'framing-explicit',states});
  });
  await evaluate("ab.session.viewPanelOpen=false;ab.view.mode='viewer'");await settle();
  await check('Saved layouts gain Auto and keep their existing order and hidden groups',async()=>{
    const legacy=['clear','rating','flags','moveCopy','labels','tags'];
    await evaluate(`localStorage.setItem('cullant.bottomBar',${JSON.stringify(JSON.stringify({order:legacy,hidden:['labels']}))})`);await send('Page.reload');await bootstrap();
    const layout=await evaluate('({order:ab.settings.bottomBarOrder,hidden:ab.settings.bottomBarHidden})');observations.push({case:'legacy-layout',layout});
    assert(layout.order.includes('auto'),'A saved layout does not gain Auto');
    assert(JSON.stringify(layout.order.filter(id=>id!=='auto'))===JSON.stringify(legacy)&&layout.hidden.includes('labels'),'Adding Auto changes existing groups or visibility');
    await settingsPanel();
    const index=await evaluate("ab.settings.bottomBarOrder.indexOf('auto')");await evaluate(`document.querySelectorAll('.draglist .handle')[${index}].focus()`);
    for(let i=0;i<index;i++)await key('ArrowUp','ArrowUp',38);
    await pause(180);
    await click('.draglist .drow:first-child input[type=checkbox]');await closeSettings();
    assert(!await evaluate("document.querySelector('.touchbar [aria-label=\"Auto\"]')"),'Hidden Auto still renders');
    await send('Page.reload');await bootstrap();
    assert(await evaluate("ab.settings.bottomBarOrder[0]==='auto' && ab.settings.bottomBarHidden.includes('auto')"),'Auto order or hidden state is lost after reload');
    await settingsPanel();await click('.draglist .drow:first-child input[type=checkbox]');await closeSettings();
    assert(await evaluate("!!document.querySelector('.touchbar [aria-label=\"Auto\"]')"),'Showing Auto does not restore its action');
  });
  await closeSettings();await evaluate('ab.settings.resetBottomBar()');await settle();
  await check('Compact bar controls leave two header rows at 400px and preserve Loupe navigation',async()=>{
    await send('Emulation.setTouchEmulationEnabled',{enabled:true});
    await send('Emulation.setDeviceMetricsOverride',{width:400,height:900,deviceScaleFactor:1,mobile:true});await settle();
    const layout=await evaluate("(()=>{const buttons=[...document.querySelectorAll('.toolbar-right>button,.toolbar-right>.filters-anchor>button,.toolbar-right>.view-anchor>button')].filter(el=>el.getBoundingClientRect().width>0);return{rows:[...new Set(buttons.map(el=>Math.round(el.getBoundingClientRect().top)))],search:!!document.querySelector('.toolbar [aria-label=\"Search filenames\"]'),loupe:!!document.querySelector('.toolbar [aria-label=Loupe]'),auto:document.querySelector('.touchbar [aria-label=\"Auto\"]')?.textContent.trim(),move:document.querySelector('.touchbar [aria-label=\"Move or copy to folder\"]')?.textContent.trim()};})()");
    observations.push({case:'narrow-layout',layout});
    assert(!layout.search&&layout.loupe,'The Search icon remains or Loupe navigation is missing');
    assert(layout.rows.length===1,'The right toolbar wraps into another row');
    assert(layout.auto==='Auto'&&layout.move==='','Auto or Move / Copy still has a wide text label');await screenshot('narrow-bar');
  });
  await send('Emulation.clearDeviceMetricsOverride');await send('Emulation.setTouchEmulationEnabled',{enabled:false});await settle();
  await check('Compare keeps action and photo zoom targets separate without repeated text badges',async()=>{
    await fixtureRows();
    assert(await evaluate("!document.querySelector('.selected-target,.zoom-target,.selection-indicator,.zoom-indicator')"),'Compare still shows action or zoom indicators beside the filenames');
    await evaluate("document.querySelectorAll('.compare .pane')[1].dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,pointerType:'mouse',button:0}))");await settle();
    assert(await evaluate("document.querySelectorAll('.compare .pane')[0].classList.contains('classification-target') && document.querySelectorAll('.compare .pane')[1].classList.contains('zoom-active')"),'Choosing the right zoom target moves the classification target');
    await click('.compare .pane.classification-target .pin-btn');
    assert(await evaluate("document.querySelectorAll('.compare .pane')[1].classList.contains('classification-target') && !document.querySelector('.selection-indicator,.zoom-indicator')"),'Pinning the action target does not move its outline to the unpinned pane');await screenshot('photo-compare');
  });
  await check('Video compare omits photo zoom markers and sync controls',async()=>{
    await fixtureRows(true);
    assert(await evaluate("document.querySelectorAll('.compare .pane').length===2 && !document.querySelector('.compare .zoom-active,.compare .zoom-target,.compare .zoom-indicator,.compare .sync-btn')"),'Video compare shows photo-only zoom controls');
    await screenshot('video-compare');
  });
  await fixtureRows();await evaluate("ab.view.mode='viewer'");await settle();
  await check('Every top and bottom bar button exposes a visible mouse tooltip',async()=>{
    const buttons=await evaluate("[...document.querySelectorAll('.toolbar button,.touchbar button')].filter(el=>!el.disabled && el.getBoundingClientRect().width>0).map((el,index)=>{el.dataset.e2eTooltip=String(index);return{selector:'[data-e2e-tooltip=\"'+index+'\"]',label:el.getAttribute('aria-label')??el.title??el.textContent.trim()};})");
    const missing=[];
    for(const button of buttons){const text=await tooltipFor(button.selector);if(!text)missing.push(button.label);}
    observations.push({case:'tooltip-coverage',checked:buttons.length,missing});assert(buttons.length>=20&&missing.length===0,`Visible tooltips missing for ${missing.join(', ')}`);await screenshot('hover-tooltip');
  });
  await check('Keyboard focus shows a tooltip and Escape dismisses it without leaving the viewer',async()=>{
    await mouseMove(null);await key('Tab','Tab',9);await evaluate("document.querySelector('.touchbar [aria-label=\"Auto\"]').focus()");await pause(700);
    assert(await evaluate("!!document.querySelector('[role=tooltip]')"),'Keyboard focus has no visible tooltip');await key('Escape','Escape',27);
    assert(await evaluate("!document.querySelector('[role=tooltip]') && ab.view.mode==='viewer'"),'Escape leaves the viewer or keeps the tooltip');
  });
  await evaluate("ab.view.mode='viewer';ab.autoCalls=0;ab.settings.setFastCulling=function(enabled){ab.autoCalls++;ab.setFast.call(this,enabled);};ab.session.autoAdvancePref=false;ab.session.capsLockActive=false");
  await send('Emulation.setTouchEmulationEnabled',{enabled:true});
  await check('Touch hold explains Auto without activating it and a short tap acts once',async()=>{
    const selector='.touchbar [aria-label="Auto"]';await mouseMove(null);await evaluate('ab.settings.setFastCulling(false);ab.autoCalls=0');
    await touch(selector,750);
    observations.push({case:'touch-hold',state:await evaluate("({calls:ab.autoCalls,enabled:ab.settings.fastCulling,tooltip:document.querySelector('[role=tooltip]')?.textContent.trim()??null})")});
    assert(await evaluate("ab.autoCalls===0 && !ab.settings.fastCulling && !!document.querySelector('[role=tooltip]')"),'Holding Auto activates it or fails to show its tooltip');await screenshot('touch-tooltip');
    await touch('.toolbar-center');assert(!await evaluate("document.querySelector('[role=tooltip]')"),'An outside touch keeps the tooltip');
    await touch(selector);
    assert(await evaluate("ab.autoCalls===1 && ab.settings.fastCulling && !document.querySelector('[role=tooltip]') && document.activeElement!==document.querySelector('.touchbar [aria-label=\"Auto\"]')"),'A short tap does not act exactly once or retains tooltip/focus');
  });
  await check('Touch movement cancels a pending hold and bar scrolling dismisses its tooltip',async()=>{
    const selector='.touchbar [aria-label="Auto"]';await evaluate('ab.autoCalls=0');const point=await box(selector);
    await send('Input.dispatchTouchEvent',{type:'touchStart',touchPoints:[point]});await send('Input.dispatchTouchEvent',{type:'touchMove',touchPoints:[{...point,x:point.x+35}]});await pause(750);await send('Input.dispatchTouchEvent',{type:'touchEnd',touchPoints:[]});await pause(100);
    assert(await evaluate("ab.autoCalls===0 && !document.querySelector('[role=tooltip]')"),'A scrolling gesture activates Auto or opens a tooltip');
    await touch(selector,750);assert(await evaluate("!!document.querySelector('[role=tooltip]')"),'A touch hold does not open the tooltip to test scrolling');
    await evaluate("document.querySelector('.touchbar').dispatchEvent(new Event('scroll'))");await settle();
    assert(!await evaluate("document.querySelector('[role=tooltip]')"),'Scrolling leaves a tooltip on screen');
  });
  await screenshot('final');
}catch(error){results.push({name:'Harness setup',status:'FAIL',error:error.stack??error.message});console.error(error);}
finally{
  await send('Emulation.setTouchEmulationEnabled',{enabled:false}).catch(()=>{});await send('Emulation.clearDeviceMetricsOverride').catch(()=>{});
  if(original){
    await evaluate(`document.querySelector('.dialog .close-x')?.click();ab.view.infoTip=null;ab.session.viewPanelOpen=false;ab.catalog.items=${JSON.stringify(original.items)};ab.catalog.media=${JSON.stringify(original.media)};ab.catalog.mediaCounts=${JSON.stringify(original.counts)};ab.settings.setFastCulling=ab.setFast;ab.session.clearFilters();ab.view.mode='grid';for(const key of Object.keys(localStorage))if(key.startsWith('cullant.'))localStorage.removeItem(key);for(const [key,value]of Object.entries(${JSON.stringify(original.storage)}))localStorage.setItem(key,value)`).catch(()=>{});
  }
  await writeFile(join(output,'results.json'),JSON.stringify({date:new Date().toISOString(),target:target.url,results,observations,runtimeErrors},null,2));
  await send('Page.reload').catch(()=>{});socket.close();
}
process.exitCode=results.some(result=>result.status==='FAIL')?1:0;
