import { listen } from "@tauri-apps/api/event";
import { currentMediaProjectRoot } from "./api";
import { IS_MOBILE } from "./platform";

/**
 * Grid thumbnails, kept in the page once they have arrived.
 *
 * A virtual grid unmounts the cells that scroll away, and a plain `<img>` that
 * mounts again asks the backend from scratch. On Android that request is never
 * served from the WebView's cache (wry forces `no-store`), and it queues behind
 * whatever the WebView's few loader threads are already waiting on -- during an
 * import, decodes. So a thumbnail the user had already seen came back as an
 * empty cell. Here each thumbnail is fetched once, held as an object URL, and a
 * remounted cell paints from memory without touching the backend.
 *
 * The loader also owns the waiting: a 503 means "being generated" and is retried
 * when `thumbs:ready` names the file, or after a backoff; a 404 is retried a few
 * times (the pool may have dropped a stale request); a 410 means the URL itself
 * is out of date and a fresh one will come with the next catalogue refresh.
 */

const BUDGET_BYTES = (IS_MOBILE ? 64 : 192) * 1024 * 1024;
const MAX_FAILURES = 4;
const BACKOFF_MS = [500, 1000, 2000, 4000, 8000];
const PENDING_RETRY_MS = 15000;

class ThumbError extends Error {
  constructor(readonly status: number) {
    super(`thumbnail request failed: ${status}`);
  }
}

interface Entry {
  objectUrl: string;
  bytes: number;
}

const entries = new Map<string, Entry>();
const inflight = new Map<string, Promise<string>>();
let totalBytes = 0;
let epoch = 0;

function touch(url: string): string | undefined {
  const entry = entries.get(url);
  if (!entry) return undefined;
  // Map iteration order is insertion order, so re-inserting marks it fresh.
  entries.delete(url);
  entries.set(url, entry);
  return entry.objectUrl;
}

function store(url: string, blob: Blob): string {
  const objectUrl = URL.createObjectURL(blob);
  entries.set(url, { objectUrl, bytes: blob.size });
  totalBytes += blob.size;
  for (const [key, entry] of entries) {
    if (totalBytes <= BUDGET_BYTES) break;
    entries.delete(key);
    totalBytes -= entry.bytes;
    URL.revokeObjectURL(entry.objectUrl);
  }
  return objectUrl;
}

function fetchThumb(url: string): Promise<string> {
  const running = inflight.get(url);
  if (running) return running;
  const started = epoch;
  const request = fetch(url)
    .then(async (response) => {
      if (!response.ok) throw new ThumbError(response.status);
      const blob = await response.blob();
      if (started !== epoch) throw new ThumbError(410);
      return store(url, blob);
    })
    .finally(() => inflight.delete(url));
  inflight.set(url, request);
  return request;
}

/** Forget every held thumbnail. File ids are only unique within one project. */
export function clearThumbCache() {
  epoch++;
  for (const entry of entries.values()) URL.revokeObjectURL(entry.objectUrl);
  entries.clear();
  inflight.clear();
  totalBytes = 0;
}

const wakers = new Map<number, Set<() => void>>();

function onReady(id: number, wake: () => void): () => void {
  let set = wakers.get(id);
  if (!set) wakers.set(id, (set = new Set()));
  set.add(wake);
  return () => {
    set.delete(wake);
    if (set.size === 0) wakers.delete(id);
  };
}

void listen<{ projectRoot: string; ids: number[] }>("thumbs:ready", (event) => {
  if (event.payload.projectRoot !== currentMediaProjectRoot()) return;
  for (const id of event.payload.ids) {
    for (const wake of [...(wakers.get(id) ?? [])]) wake();
  }
});

export type ThumbState = "loading" | "pending" | "ready" | "failed";

export interface GridThumbParams {
  url: string;
  id: number;
  /** The catalogue says this thumbnail already exists, so a wait is short. */
  exists: boolean;
  onload?: (width: number, height: number) => void;
  /** Called once the loader stops retrying. */
  onfail?: () => void;
  /** Failed attempts before giving up; a 503 "being generated" never counts. */
  retries?: number;
}

/**
 * Svelte action for a grid `<img>`. It writes the load state to the image's
 * parent as `data-thumb`, so the cell can draw its placeholder from CSS.
 */
export function gridThumb(image: HTMLImageElement, initial: GridThumbParams) {
  let params = initial;
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let unsubscribe: (() => void) | undefined;
  let failures = 0;

  const setState = (state: ThumbState) => {
    if (image.parentElement) image.parentElement.dataset.thumb = state;
  };

  const settle = () => {
    clearTimeout(timer);
    timer = undefined;
    unsubscribe?.();
    unsubscribe = undefined;
  };

  image.onload = () => {
    setState("ready");
    params.onload?.(image.naturalWidth, image.naturalHeight);
  };
  image.onerror = () => {
    // A held object URL that no longer decodes (revoked under us): drop it and
    // fetch again rather than leaving a broken image.
    if (!image.src.startsWith("blob:")) return;
    entries.delete(params.url);
    image.removeAttribute("src");
    void attempt();
  };

  async function attempt() {
    const mine = ++generation;
    settle();
    const held = touch(params.url);
    if (held) {
      if (image.src !== held) {
        setState("loading");
        image.src = held;
      }
      return;
    }
    setState(params.exists ? "loading" : "pending");
    try {
      const objectUrl = await fetchThumb(params.url);
      if (mine !== generation) return;
      image.src = objectUrl;
    } catch (error) {
      if (mine !== generation) return;
      const status = error instanceof ThumbError ? error.status : 0;
      if (status === 410) {
        setState("failed");
        return;
      }
      if (status === 503) {
        // Being generated. The backend names the file when it lands; the timer
        // only covers an event that was missed.
        setState("pending");
        unsubscribe = onReady(params.id, () => void attempt());
        timer = setTimeout(() => void attempt(), PENDING_RETRY_MS);
        return;
      }
      failures++;
      if (failures > (params.retries ?? MAX_FAILURES)) {
        setState("failed");
        params.onfail?.();
        return;
      }
      unsubscribe = onReady(params.id, () => void attempt());
      timer = setTimeout(
        () => void attempt(),
        BACKOFF_MS[Math.min(failures - 1, BACKOFF_MS.length - 1)],
      );
    }
  }

  void attempt();

  return {
    update(next: GridThumbParams) {
      const changed = next.url !== params.url;
      params = next;
      if (changed) {
        failures = 0;
        void attempt();
      }
    },
    destroy() {
      generation++;
      settle();
      image.onload = image.onerror = null;
    },
  };
}
