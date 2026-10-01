import {readFile,mkdir,writeFile} from "node:fs/promises";
import {createHash} from "node:crypto";
import {resolve,join} from "node:path";

// Failure modes: Android applies Range twice to an intercepted partial body;
// the explicit query range is ignored; bytes or Content-Range are incorrect.
const port=process.env.CULLANT_CDP_PORT??"9223";
const output=resolve(process.argv[2]??".playwright-mcp/video-range");
const file=resolve(process.argv[3]??".playwright-mcp/android-video-camera/DSCF5044.MOV");
const source=await readFile(file),results=[];
const targets=await(await fetch("http://127.0.0.1:"+port+"/json/list")).json();
const target=targets.find(page=>page.type==="page"&&/tauri\.localhost|localhost:1420/.test(page.url));
if(!target)throw new Error("The Cullant WebView was not found");
const socket=new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve,reject)=>{socket.onopen=resolve;socket.onerror=reject;});
const pending=new Map();let sequence=0;
socket.onmessage=({data})=>{const reply=JSON.parse(data),task=pending.get(reply.id);if(!task)return;pending.delete(reply.id);if(reply.error)task.reject(new Error(reply.error.message));else task.resolve(reply.result);};
function send(method,params){return new Promise((resolve,reject)=>{const id=++sequence;pending.set(id,{resolve,reject});socket.send(JSON.stringify({id,method,params}));});}
async function evaluate(fn,arg){const reply=await send("Runtime.evaluate",{expression:"("+fn.toString()+")("+JSON.stringify(arg??null)+")",awaitPromise:true,returnByValue:true});if(reply.exceptionDetails)throw new Error(reply.exceptionDetails.exception?.description??reply.exceptionDetails.text);return reply.result.value;}
await mkdir(output,{recursive:true});
try{
  const url=process.env.CULLANT_VIDEO_URL??await evaluate(()=>{
    const video=document.querySelector("video.player")?.currentSrc;
    if(video?.includes("/video/"))return video;
    const poster=document.querySelector(".video-poster img")?.currentSrc;
    return poster?.replace("/preview/","/video/").replace("/thumb/","/video/");
  });
  if(!url?.includes("/video/"))throw new Error("Open the reproduction clip before running this check");
  for(const [start,length] of [[0,1],[524288,64],[9000000,64],[source.length-4096,4096]]){
    const result=await evaluate(async({url,start,length})=>{
      try{
        const response=await fetch(url+"&range="+encodeURIComponent(`bytes=${start}-${start+length-1}`),{signal:AbortSignal.timeout(3000)});
        const bytes=await response.arrayBuffer();
        const hash=await crypto.subtle.digest("SHA-256",bytes);
        return{status:response.status,range:response.headers.get("Content-Range"),length:bytes.byteLength,sha256:[...new Uint8Array(hash)].map(v=>v.toString(16).padStart(2,"0")).join("")};
      }catch(error){return{error:error.message};}
    },{url,start,length});
    const expected={range:`bytes ${start}-${start+length-1}/${source.length}`,length,sha256:createHash("sha256").update(source.subarray(start,start+length)).digest("hex")};
    const pass=result.status===206&&result.range===expected.range&&result.length===length&&result.sha256===expected.sha256;
    results.push({name:`Video bytes ${start}-${start+length-1}`,status:pass?"PASS":"FAIL",result,expected});
    console.log((pass?"PASS":"FAIL")+": "+results.at(-1).name);
  }
}catch(error){results.push({name:"Range setup",status:"FAIL",error:error.stack??error.message});}
finally{await writeFile(join(output,"results.json"),JSON.stringify({date:new Date().toISOString(),target:target.url,source:file,results},null,2));socket.close();}
process.exitCode=results.some(result=>result.status==="FAIL")?1:0;
