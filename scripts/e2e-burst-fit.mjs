import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const output=resolve(process.argv[2]??'artifacts/performance/burst-fit');
await mkdir(output,{recursive:true});
const targets=await(await fetch('http://127.0.0.1:9223/json/list')).json();
const target=targets.find(t=>t.type==='page');
assert(target,'Start the isolated app with D: and Vite on port 1420');
const socket=new WebSocket(target.webSocketDebuggerUrl);
await new Promise((ok,fail)=>{socket.onopen=ok;socket.onerror=fail;});
let seq=0;
const pending=new Map();
socket.onmessage=({data})=>{const row=JSON.parse(data),task=pending.get(row.id);if(!task)return;pending.delete(row.id);clearTimeout(task.timer);row.error?task.fail(new Error(row.error.message)):task.ok(row.result);};
function send(method,params={}){return new Promise((ok,fail)=>{const id=++seq,timer=setTimeout(()=>fail(new Error(method+' timed out')),30000);pending.set(id,{ok,fail,timer});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(expression){const r=await send('Runtime.evaluate',{expression,awaitPromise:true,returnByValue:true});if(r.exceptionDetails)throw new Error(JSON.stringify(r.exceptionDetails));return r.result.value;}
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const observations=[];
try{
  if(process.env.CULLANT_GRID_TEST_WIDTH)await send('Emulation.setDeviceMetricsOverride',{width:Number(process.env.CULLANT_GRID_TEST_WIDTH),height:844,deviceScaleFactor:1,mobile:false});
  await send('Page.navigate',{url:'http://localhost:1420/'});
  for(let i=0;i<100;i++){if(await evaluate(`performance.getEntriesByType('resource').some(e=>new URL(e.name).pathname==='/src/lib/stores/catalog.svelte.ts')`))break;await pause(200);}
  await evaluate(`(async()=>{
    const loaded=path=>import(performance.getEntriesByType('resource').find(e=>new URL(e.name).pathname===path)?.name??path);
    const {catalog}=await loaded('/src/lib/stores/catalog.svelte.ts');
    const {settings}=await loaded('/src/lib/stores/settings.svelte.ts');
    const {session}=await loaded('/src/lib/stores/session.svelte.ts');
    const {view}=await loaded('/src/lib/stores/view.svelte.ts');
    window.burstFitProbe={settings,session,view,oldSettings:{gridPhotoFit:settings.gridPhotoFit,collapseBursts:settings.collapseBursts},oldMode:view.mode};
    settings.gridPhotoFit='fit';view.mode='grid';
  })()`);
  for(let i=0;i<100;i++){if(await evaluate(`!!document.querySelector('.viewport')`))break;await pause(200);}
  for(const fit of ['fit','fill']) for(const collapsed of [true,false]){
    await evaluate(`burstFitProbe.settings.gridPhotoFit=${JSON.stringify(fit)};burstFitProbe.settings.collapseBursts=${collapsed}`);
    for(const page of [0,1,3]){
      await evaluate(`(()=>{const el=document.querySelector('.viewport');el.scrollTop=el.clientHeight*${page};el.dispatchEvent(new Event('scroll'));})()`);
      await pause(1000);
      for(let i=0;i<100;i++){
        if(await evaluate(`[...document.querySelectorAll('.cell img')].every(i=>i.complete&&i.naturalWidth>0)`))break;
        await pause(100);
      }
      const metrics=await evaluate(`(()=>{
        return [...document.querySelectorAll('.cell .photo,.cell .deck')].map(box=>{
          const img=box.querySelector('img'),rect=box.getBoundingClientRect();
          const cell=box.closest('.cell'),frame=box.closest('.frame').getBoundingClientRect();
          const counter=box.querySelector('.burst'),icon=counter?.querySelector('svg');
          return {name:cell.querySelector('.name')?.textContent,kind:box.classList.contains('deck')?'deck':'photo',stacked:box.closest('.frame').classList.contains('stacked'),counterVisible:!!counter?.getBoundingClientRect().width&&!!icon?.getBoundingClientRect().width,frontBorder:parseFloat(getComputedStyle(box,'::after').borderTopWidth)>0,source:img?.currentSrc,w:rect.width,h:rect.height,nw:img?.naturalWidth,nh:img?.naturalHeight,fit:img&&getComputedStyle(img).objectFit,insideFrame:rect.left>=frame.left-1&&rect.right<=frame.right+1&&rect.top>=frame.top-1&&rect.bottom<=frame.bottom+1};
        });
      })()`);
      const failures=metrics.filter(r=>!r.nw||(fit==='fit'&&Math.abs(r.w/r.h-r.nw/r.nh)>.03)||r.fit!==(fit==='fit'?'contain':'cover')||!r.insideFrame||(r.kind==='photo'&&r.stacked&&(!r.counterVisible||!r.frontBorder)));
      observations.push({fit,collapsed,page,metrics,failures});
      const shot=await send('Page.captureScreenshot',{format:'png'});
      await writeFile(resolve(output,`${fit}-${collapsed?'collapsed':'expanded'}-${page}.png`),Buffer.from(shot.data,'base64'));
    }
  }
}finally{
  await evaluate(`(()=>{if(!window.burstFitProbe)return;Object.assign(burstFitProbe.settings,burstFitProbe.oldSettings);burstFitProbe.view.mode=burstFitProbe.oldMode;})()`).catch(()=>{});
  await send('Emulation.clearDeviceMetricsOverride').catch(()=>{});
  await writeFile(resolve(output,'results.json'),JSON.stringify({observations},null,2));socket.close();
}
const failures=observations.flatMap(o=>o.failures),decks=observations.flatMap(o=>o.metrics).filter(r=>r.kind==='deck');
console.log(JSON.stringify({observations:observations.length,decks:decks.length,failures:failures.length,examples:failures.slice(0,3)},null,2));
assert(decks.length,'No collapsed burst was tested');
assert.equal(failures.length,0,'Every burst image must fit its own dimensions');
