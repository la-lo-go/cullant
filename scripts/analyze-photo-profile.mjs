import { readFile, readdir, writeFile } from "node:fs/promises";
import { resolve, join } from "node:path";

const output = resolve(process.argv[2] ?? "artifacts/performance/2026-10-01-d-drive");
const json = async file => JSON.parse((await readFile(join(output, file), "utf8")).replace(/^\uFEFF/, ""));
const lines = async file => (await readFile(join(output, file), "utf8")).replace(/^\uFEFF/, "").trim().split(/\r?\n/).map(JSON.parse);
const summary = [];
for (const file of (await readdir(output)).filter(f => f.endsWith("-result.json"))) {
  const result = await json(file), run = result.run;
  const records = await lines(`${run}-profile.jsonl`).catch(() => []);
  const last = records.filter(r => r.event === "ingest_complete").at(-1);
  const stages = last?.stages ?? {};
  const stage = key => stages[key]?.totalMs ?? 0;
  const start = last ? last.unixMs - last.elapsedMs : result.start;
  const completion = result.events.filter(e => e.event === "previews:progress" && e.payload.done >= e.payload.total).at(-1)?.at;
  const end = last?.unixMs ?? completion ?? result.start + result.elapsedMs;
  const samples = (await lines(`${run}-resources.jsonl`)).filter(s => s.unixMs >= start && s.unixMs <= end);
  const appPid = last?.pid ?? samples[0]?.appPid;
  const appSamples = samples.map(s => ({ at:s.unixMs, p:s.processes.find(p => p.Id === appPid) })).filter(s => s.p);
  const first = appSamples[0], final = appSamples.at(-1);
  const cpuCores = first && final && final.at > first.at ? (final.p.CPU-first.p.CPU)/((final.at-first.at)/1000) : null;
  const buckets = {
    decode: stage("decode.image") + stage("decode.paired_jpeg") + stage("decode.raw"),
    resize: stage("resize.preview") + stage("resize.thumb"),
    orientation: stage("orientation.preview") + stage("orientation.thumb"),
    encode: stage("jpeg_encode.preview") + stage("jpeg_encode.thumb"),
    write: stage("cache_write"),
    hash: stage("dhash"),
  };
  const gpuFraction = stage("worker") ? (buckets.resize+buckets.orientation)/stage("worker") : null;
  summary.push({
    run, edge:result.edge, backendSeconds:last?.elapsedMs/1000 || null,
    observedSeconds:result.elapsedMs/1000, firstGridSeconds:result.firstGridMs === null ? null : (result.start+result.firstGridMs-start)/1000,
    metadataSeconds:stage("metadata")/1000, thumbnailPassSeconds:stage("pass.thumb")/1000, previewPassSeconds:stage("pass.preview")/1000,
    workerSeconds:stage("worker")/1000, bucketsSeconds:Object.fromEntries(Object.entries(buckets).map(([k,v])=>[k,v/1000])),
    transformWorkerFraction:gpuFraction, illustrativeZeroCostTransformSpeedup:gpuFraction === null ? null : 1/(1-gpuFraction),
    appAverageCpuCores:cpuCores, appAverageCpuPercentOf16:cpuCores===null?null:cpuCores/16*100,
    appPeakWorkingSetMiB:appSamples.length?Math.max(...appSamples.map(s=>s.p.WorkingSet64))/1048576:null,
    appPeakPrivateMiB:appSamples.length?Math.max(...appSamples.map(s=>s.p.PrivateMemorySize64))/1048576:null,
    peakDiskReadMiBps:samples.length?Math.max(...samples.flatMap(s=>s.disk.map(d=>Number(d.DiskReadBytesPersec))))/1048576:null,
    peakDiskWriteMiBps:samples.length?Math.max(...samples.flatMap(s=>s.disk.map(d=>Number(d.DiskWriteBytesPersec))))/1048576:null,
    resourceSamples:samples.length, sourceDecodeCounts:Object.fromEntries(["decode.image","decode.paired_jpeg","decode.raw"].map(k=>[k,stages[k]?.count??0])),
    thumbnailsEncoded:stages["jpeg_encode.thumb"]?.count??null,previewsEncoded:stages["jpeg_encode.preview"]?.count??null,
    artifacts:result.artifacts.length, status:result.status,
  });
}
await writeFile(join(output,"summary.json"),JSON.stringify(summary,null,2));
console.log(JSON.stringify(summary,null,2));
