// Backend regression pins the pre-metadata window. This checks the real protocol,
// decoded pixels, cache headers, and loupe in the running isolated app.
import assert from 'node:assert/strict';
import { mkdir, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { DatabaseSync } from 'node:sqlite';

const root = resolve('.playwright-mcp/first-orientation-project');
const output = resolve(process.argv[2] ?? 'artifacts/performance/orientation-fix');
await mkdir(output, { recursive: true });
if (process.argv.includes('--prepare')) {
  execFileSync('python', ['-c', `
from pathlib import Path
from PIL import Image
p=Path(${JSON.stringify(root.replaceAll('\\', '/'))})
assert not p.exists(), 'Use a new disposable fixture directory'
p.mkdir(parents=True)
im=Image.new('RGB',(80,60))
for y in range(60):
 for x in range(80): im.putpixel((x,y),(240 if x<40 else 20,240 if y<30 else 20,30))
for orientation in range(1,9):
 exif=Image.Exif();exif[274]=orientation
 im.save(p/f'orientation-{orientation}.jpg',quality=95,exif=exif if orientation!=1 else b'')
`]);
  console.log(root);
  process.exit(0);
}
const targets = await (await fetch('http://127.0.0.1:9223/json/list')).json();
const target = targets.find(t => t.type === 'page');
assert(target, 'Start the isolated app on CDP 9223 with the fixture project');
const socket = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((ok, fail) => { socket.onopen = ok; socket.onerror = fail; });
let seq = 0;
const pending = new Map();
const held = [];
socket.onmessage = ({data}) => {
  const reply = JSON.parse(data);
  if (reply.method === 'Fetch.requestPaused') { held.push(reply.params.requestId); return; }
  const task = pending.get(reply.id);
  if (!task) return;
  clearTimeout(task.timer); pending.delete(reply.id);
  reply.error ? task.fail(new Error(reply.error.message)) : task.ok(reply.result);
};
function send(method, params = {}) {
  return new Promise((ok, fail) => {
    const id = ++seq, timer = setTimeout(() => fail(new Error(method+' timed out')), 30000);
    pending.set(id, {ok, fail, timer}); socket.send(JSON.stringify({id, method, params}));
  });
}
async function evaluate(expression) {
  const result = await send('Runtime.evaluate', {expression, awaitPromise:true, returnByValue:true});
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? JSON.stringify(result.exceptionDetails));
  return result.result.value;
}
try {
  await send('Page.navigate',{url:'http://localhost:1420/'});
  for(let i=0;i<100;i++){
    if(await evaluate(`document.querySelectorAll('.cell img').length===8`))break;
    await new Promise(r=>setTimeout(r,200));
  }
  // Pin the pre-metadata state in this disposable project after import settles.
  const db = new DatabaseSync(resolve(root, '.cullant/cullant.db'));
  try {
    assert.equal(resolve(db.prepare('SELECT root_path FROM project WHERE id=1').get().root_path), root);
    assert.equal(db.prepare('SELECT count(*) AS n FROM files').get().n, 8);
    assert.equal(db.prepare('SELECT count(*) AS n FROM files WHERE capture_time IS NULL').get().n, 0);
    assert.equal(db.prepare("SELECT orientation FROM files WHERE rel_path='orientation-1.jpg'").get().orientation, 1, 'Metadata must resolve an absent tag to orientation 1');
    db.exec('UPDATE files SET orientation=NULL');
  } finally { db.close(); }
  const result = await evaluate(`(async()=>{
    const invoke=window.__TAURI_INTERNALS__.invoke;
    const rows=await invoke('query_items',{media:'photos',sort:'capture',desc:false});
    if(rows.length!==8 || !rows.every(r=>/^orientation-[1-8]\\.jpg$/.test(r.relPath)))throw Error('Use only the orientation fixture project');
    const results=[];
    for(const item of rows){
      const orientation=Number(item.relPath.match(/[1-8]/)[0]);
      for(const kind of ['thumb','preview','full']){
        for(const version of [0,orientation]){
          const response=await fetch('http://cullant.localhost/'+kind+'/'+item.id+'?v='+item.mtime+'&o='+version+'&p='+encodeURIComponent(${JSON.stringify(root)}));
          if(!response.ok)throw Error('Image request failed: '+response.status);
          const bitmap=await createImageBitmap(await response.blob());
          const canvas=document.createElement('canvas');canvas.width=bitmap.width;canvas.height=bitmap.height;
          const ctx=canvas.getContext('2d');ctx.drawImage(bitmap,0,0);
          results.push({orientation,kind,version,width:bitmap.width,height:bitmap.height,cache:response.headers.get('cache-control'),corner:[...ctx.getImageData(10,10,1,1).data]});bitmap.close();
        }
      }
    }
    return results;
  })()`);
  const corners = [[240,240],[20,240],[20,20],[240,20],[240,240],[240,20],[20,20],[20,240]];
  for (const row of result) {
    assert.deepEqual([row.width,row.height], row.orientation>=5?[60,80]:[80,60]);
    assert(row.version===0 ? row.cache==='no-store' : row.cache.includes('immutable'));
    corners[row.orientation-1].forEach((value,c)=>assert(Math.abs(row.corner[c]-value)<25, JSON.stringify(row)));
  }
  if (process.argv.includes('--transition')) {
    await evaluate(`(async()=>{
      const loaded=path=>import(performance.getEntriesByType('resource').find(e=>new URL(e.name).pathname===path)?.name??path);
      const {catalog}=await loaded('/src/lib/stores/catalog.svelte.ts');
      const {view}=await loaded('/src/lib/stores/view.svelte.ts');
      const {settings}=await loaded('/src/lib/stores/settings.svelte.ts');
      const {session}=await loaded('/src/lib/stores/session.svelte.ts');
      settings.collapseBursts=false;session.clearFilters();session.groupBy=[];
      view.mode='grid';window.orientationProbe={catalog};
      catalog.items=catalog.items.map(item=>({...item,orientation:null}));
      catalog.previewReady.clear();
    })()`);
    await new Promise(r=>setTimeout(r,500));
  }
  await evaluate(`(async()=>{const items=await window.__TAURI_INTERNALS__.invoke('query_items',{media:'photos',sort:'capture',desc:false});const id=items.find(i=>i.relPath==='orientation-6.jpg').id;const cell=[...document.querySelectorAll('.cell')].find(c=>c.querySelector('img')?.src.includes('/thumb/'+id+'?'));if(!cell)throw Error('Portrait cell missing');const box=cell.getBoundingClientRect();cell.dispatchEvent(new MouseEvent('dblclick',{bubbles:true,clientX:box.x+box.width/2,clientY:box.y+box.height/2}));})()`);
  let loupe;
  for(let i=0;i<100;i++){
    loupe=await evaluate(`(()=>{const img=document.querySelector('.viewer img.fit');return img&&{width:img.naturalWidth,height:img.naturalHeight,complete:img.complete,transform:getComputedStyle(img).transform};})()`);
    if(loupe?.complete&&loupe.height>loupe.width)break;
    await new Promise(r=>setTimeout(r,100));
  }
  assert(loupe?.complete&&loupe.height>loupe.width, 'Portrait loupe did not load');
  let transition = null;
  if (process.argv.includes('--transition')) {
    await send('Fetch.enable',{patterns:[{urlPattern:'*cullant.localhost/preview/*',requestStage:'Request'}]});
    await evaluate(`orientationProbe.catalog.items=orientationProbe.catalog.items.map(item=>({...item,orientation:Number(item.relPath.match(/[1-8]/)[0])}))`);
    for(let i=0;i<50&&!held.length;i++)await new Promise(r=>setTimeout(r,100));
    assert(held.length, 'Replacement preview was not requested');
    transition=await evaluate(`(()=>{const img=document.querySelector('.viewer img.fit');return {turning:img.classList.contains('turning'),source:img.currentSrc,width:img.naturalWidth,height:img.naturalHeight};})()`);
    assert(transition.source.includes('&o=0&'), 'Expected the provisional image to remain visible');
    assert.equal(transition.turning,false,'Metadata rotated already oriented pixels a second time');
    assert(transition.height>transition.width);
    for(const requestId of held.splice(0))await send('Fetch.continueRequest',{requestId});
    await send('Fetch.disable');
  }
  const shot=await send('Page.captureScreenshot',{format:'png'});
  await writeFile(resolve(output,'first-orientation.png'),Buffer.from(shot.data,'base64'));
  await writeFile(resolve(output,'first-orientation.json'),JSON.stringify({status:'PASS',requests:result,loupe,transition},null,2));
  console.log(JSON.stringify({status:'PASS',requests:result.length,loupe,transition}));
} finally {
  await send('Fetch.disable').catch(()=>{});
  socket.close();
}
