// Failure modes: missing phases, overlapping timers mistaken for wall time,
// disabled profiling writing files, and partial image generation.
import { readFile } from "node:fs/promises";
import assert from "node:assert/strict";

const records = (await readFile(process.argv[2], "utf8")).trim().split(/\r?\n/).map(JSON.parse);
assert(records.length > 0);
assert(records.every(row => row.schema === 1 && row.elapsedMs >= 0));
const last = records.at(-1);
for (const key of ["scan", "metadata", "resize.preview", "resize.thumb", "jpeg_encode.preview", "jpeg_encode.thumb", "cache_write", "db.write_wait", "source_open"]) {
  assert(last.stages[key]?.count > 0, `Missing stage: ${key}`);
  assert(last.stages[key].totalMs >= last.stages[key].maxMs, `Invalid duration: ${key}`);
}
assert(records.some(row => row.event === "ingest_complete"), "Ingest did not finish");
console.log(JSON.stringify({ status: "PASS", records: records.length, elapsedMs: last.elapsedMs, stages: Object.keys(last.stages).length }));
