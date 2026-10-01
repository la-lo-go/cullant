import {mkdir,writeFile} from "node:fs/promises";
import {resolve,join} from "node:path";
import {execFileSync} from "node:child_process";

// Failure modes: a native capability hint delays failure until Play; a failed
// clip reopens without its warning; cached failure still reads the video.
const output=resolve(process.argv[2]??".playwright-mcp/android-video-memory");
const filename=(process.argv[3]??"DSCF5044.MOV").toLowerCase();
const playable=process.env.CULLANT_EXPECT_PLAYABLE==="1";
const forceUnsupported=process.env.CULLANT_FORCE_UNSUPPORTED==="1";
const targets=await(await fetch("http://127.0.0.1:9223/json/list")).json();
const target=targets.find(page=>page.type==="page"&&page.url.includes("tauri.localhost"));
if(!target)throw new Error("Connect the diagnostic Cullant WebView to CDP port 9223.");
const socket=new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
const pending=new Map(),requests=[],responses=[],logs=[],results=[];
let sequence=0;
socket.onmessage=({data})=>{
  const reply=JSON.parse(data),task=pending.get(reply.id);
  if(reply.method==="Network.requestWillBeSent"&&new URL(reply.params.request.url).pathname.startsWith("/video/"))requests.push({url:reply.params.request.url,type:reply.params.type});
  if(reply.method==="Network.responseReceived"&&new URL(reply.params.response.url).pathname.startsWith("/video/"))responses.push({url:reply.params.response.url,type:reply.params.type,status:reply.params.response.status,headers:reply.params.response.headers});
  if(reply.method==="Runtime.consoleAPICalled")logs.push({type:reply.params.type,args:reply.params.args.map(arg=>arg.value??arg.description)});
  if(!task)return;
  pending.delete(reply.id);
  if(reply.error)task.reject(new Error(reply.error.message));else task.resolve(reply.result);
};
function send(method,params={}){return new Promise((resolve,reject)=>{const id=++sequence;const timer=setTimeout(()=>{pending.delete(id);reject(new Error("CDP timed out: "+method));},10000);pending.set(id,{resolve:value=>{clearTimeout(timer);resolve(value);},reject:error=>{clearTimeout(timer);reject(error);}});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(fn,arg){const reply=await send("Runtime.evaluate",{expression:"("+fn.toString()+")("+JSON.stringify(arg??null)+")",awaitPromise:true,returnByValue:true});if(reply.exceptionDetails)throw new Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);return reply.result.value;}
const pause=ms=>new Promise(resolve=>setTimeout(resolve,ms));
async function waitFor(fn,timeout=8000){const deadline=Date.now()+timeout;while(Date.now()<deadline){if(await evaluate(fn))return true;await pause(50);}return false;}
async function screenshot(name){
  const adb=process.env.CULLANT_ADB??"C:/Android/sdk/platform-tools/adb.exe",device=process.env.CULLANT_ADB_DEVICE??"100.89.241.106:45353";
  const focus=execFileSync(adb,["-s",device,"shell","dumpsys","window"],{encoding:"utf8"}).split("\n").find(line=>line.includes("mCurrentFocus="));
  if(!focus?.includes("org.cullant.app"))throw new Error("Cullant left the foreground");
  const shot=execFileSync(adb,["-s",device,"exec-out","screencap","-p"],{maxBuffer:16*1024*1024});
  await writeFile(join(output,name+".png"),shot);
}
async function state(){return evaluate(()=>{const v=document.querySelector("video.player"),p=document.querySelector(".video-poster img");return{warning:!!document.querySelector(".fallback"),source:v?.currentSrc,paused:v?.paused,ready:v?.readyState,error:v?.error?.code,errorMessage:v?.error?.message,time:v?.currentTime,poster:p&&{src:p.currentSrc,width:p.naturalWidth},controls:[...document.querySelectorAll(".controls button,.controls input")].map(c=>({title:c.title,disabled:c.disabled}))};});}
async function tap(point){await send("Input.dispatchTouchEvent",{type:"touchStart",touchPoints:[point]});await send("Input.dispatchTouchEvent",{type:"touchEnd",touchPoints:[]});}
async function openClip(){
  await evaluate(()=>{document.querySelector('button[aria-label="Videos"]')?.click();document.querySelector('button[aria-label="Grid"]')?.click();});
  if(!await waitFor(()=>!!document.querySelector(".grid-root .cell")))throw new Error("The video grid is not open");
  const point=await evaluate(filename=>{const cell=[...document.querySelectorAll(".grid-root .cell")].find(c=>c.textContent.toLowerCase().includes(filename));if(!cell)return null;const r=cell.getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2};},filename);
  if(!point)throw new Error("The video grid does not contain "+filename);
  await tap(point);
  if(!await waitFor(()=>!!document.querySelector("video.player")))throw new Error("The video did not open");
}
function check(name,condition,details){results.push({name,status:condition?"PASS":"FAIL",details});console.log((condition?"PASS":"FAIL")+": "+name);}
await mkdir(output,{recursive:true});
try{
  await send("Runtime.enable");await send("Network.enable");await send("Page.enable");
  if(forceUnsupported)await evaluate(()=>{
    window.originalVideoSupport=MediaSource.isTypeSupported;MediaSource.isTypeSupported=()=>false;
    window.originalNativeSupport=HTMLMediaElement.prototype.canPlayType;HTMLMediaElement.prototype.canPlayType=()=>"";
  });
  await openClip();
  if(playable){
    const ready=await waitFor(()=>document.querySelector("video.player")?.readyState>=2&&document.querySelector("video.player")?.src.startsWith("blob:")&&!document.querySelector(".fallback"),20000);
    const prepared=await state();await screenshot("prepared");
    check("The camera clip is prepared in app without autoplay",ready&&prepared.paused&&prepared.time===0,prepared);
    const point=await evaluate(()=>{const r=document.querySelector("video.player").getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2};});
    await tap(point);
    const playing=await waitFor(()=>document.querySelector("video.player")?.currentTime>0.2&&!document.querySelector(".video-poster")&&!document.querySelector(".fallback"));
    const played=await state();await screenshot("playing");
    check("The real camera clip presents video frames",playing,played);
    await evaluate(()=>{const v=document.querySelector("video.player");v.pause();v.currentTime=1.5;});
    const sought=await waitFor(()=>{const v=document.querySelector("video.player");return v&&!v.seeking&&v.currentTime>=1.4&&!v.error;});
    check("The real camera clip can seek",sought,await state());
  }else{
  const early=await waitFor(()=>!!document.querySelector(".fallback"),3000);
  const first=await state();await screenshot("initial");
  check("The camera clip warns before Play",early,first);
  if(!early){const point=await evaluate(()=>{const r=document.querySelector("video.player").getBoundingClientRect();return{x:r.left+r.width/2,y:r.top+r.height/2};});await tap(point);}
  const failed=await waitFor(()=>!!document.querySelector(".fallback"));
  const failure=await state();await screenshot("confirmed-failure");
  check("The failed clip keeps its poster and disables transport",failed&&failure.poster?.width>0&&failure.controls.every(c=>c.disabled),failure);
  const before=requests.length;
  await openClip();await pause(150);
  const reopened=await state();await pause(350);
  const newRequests=requests.slice(before);await screenshot("reopened");
  check("The camera clip reopens with its warning immediately",reopened.warning,reopened);
  check("A remembered failure makes no video requests",newRequests.length===0,newRequests);
  }
}catch(error){check("Device reproduction",false,error.stack??error.message);}
finally{
  if(forceUnsupported)await evaluate(()=>{MediaSource.isTypeSupported=originalVideoSupport;HTMLMediaElement.prototype.canPlayType=originalNativeSupport;}).catch(()=>{});
  await writeFile(join(output,"results.json"),JSON.stringify({date:new Date().toISOString(),video:filename,forcedUnsupported:forceUnsupported,expectedPlayable:playable,target:target.url,results,requests,responses,logs},null,2));socket.close();
}
process.exitCode=results.some(result=>result.status==="FAIL")?1:0;
