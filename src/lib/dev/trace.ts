//! Dev-only observation hook for the ingest pipeline.
//!
//! The stores hold only the *current* value of each phase, so a poll from
//! outside cannot see a transition it did not happen to catch. This records
//! every ingest event with a timestamp, which is what makes phase order,
//! resume-after-reopen and per-phase duration checkable from a CDP client.
//!
//! Imported dynamically behind `import.meta.env.DEV`, so the bundler drops it
//! from a production build.

import { listen } from "@tauri-apps/api/event";
import { catalog } from "$lib/stores/catalog.svelte";

type Progress = { done?: number; total?: number; ids?: number[]; found?: number };

type Milestone = { ms: number; at: string; event: string; done?: number; total?: number };

type Phase = {
  firstMs: number;
  lastMs: number;
  emits: number;
  done: number;
  total: number;
  /** Distinct ids reported complete, for the phases that carry them. */
  ids: number;
};

const MAX_MILESTONES = 500;
/** A progress event can fire many times a second; sample the trail instead of
 *  keeping every emit, so a long import cannot evict the milestones. */
const TRAIL_INTERVAL_MS = 500;

const LIFECYCLE = ["scan:done", "scan:empty", "scan:error", "metadata:done", "thumbs:done"];
const PROGRESS = [
  "scan:progress",
  "metadata:progress",
  "thumbs:progress",
  "previews:progress",
  "videos:progress",
];

const t0 = performance.now();
const milestones: Milestone[] = [];
const phases: Record<string, Phase> = {};
const trail: Milestone[] = [];
const lastTrailMs: Record<string, number> = {};

function now() {
  return Math.round(performance.now() - t0);
}

function mark(list: Milestone[], event: string, p?: Progress) {
  if (list.length >= MAX_MILESTONES) list.shift();
  list.push({
    ms: now(),
    at: new Date().toISOString().slice(11, 23),
    event,
    done: p?.done ?? p?.found,
    total: p?.total,
  });
}

function record(event: string, p: Progress) {
  const ms = now();
  const ph = (phases[event] ??= { firstMs: ms, lastMs: ms, emits: 0, done: 0, total: 0, ids: 0 });
  ph.lastMs = ms;
  ph.emits += 1;
  ph.done = p.done ?? p.found ?? ph.done;
  ph.total = p.total ?? ph.total;
  ph.ids += p.ids?.length ?? 0;

  if (ms - (lastTrailMs[event] ?? -Infinity) >= TRAIL_INTERVAL_MS) {
    lastTrailMs[event] = ms;
    mark(trail, event, p);
  }
}

export function installDevTrace() {
  for (const event of LIFECYCLE) void listen(event, () => mark(milestones, event));
  for (const event of PROGRESS) {
    void listen<Progress>(event, (e) => record(event, e.payload ?? {}));
  }

  (window as unknown as Record<string, unknown>).__cullant = {
    milestones,
    phases,
    trail,
    reset() {
      milestones.length = 0;
      trail.length = 0;
      for (const k of Object.keys(phases)) delete phases[k];
    },
    /** Live view of what the UI believes it has. `thumbLoaded` and
     *  `previewReady` are what a reopen must adopt rather than regenerate. */
    state: () => ({
      project: catalog.project?.rootPath ?? null,
      fileCount: catalog.project?.fileCount ?? 0,
      ingesting: catalog.ingesting,
      scanning: catalog.scanning,
      preloading: catalog.preloading,
      meta: { ...catalog.metaProgress },
      thumbs: { ...catalog.thumbProgress },
      previews: { ...catalog.previewProgress },
      videos: { ...catalog.videoProgress },
      thumbLoaded: catalog.thumbLoaded.size,
      previewReady: catalog.previewReady.size,
      items: catalog.items.length,
    }),
  };
}
