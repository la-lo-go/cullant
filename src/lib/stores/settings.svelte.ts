/** App-wide user preferences, persisted in localStorage. */

const PROGRESSIVE_LOUPE_KEY = "cullant.progressiveLoupe";
const REMEMBER_SESSION_KEY = "cullant.rememberSession";
const GENERATE_VIDEO_THUMBS_KEY = "cullant.generateVideoThumbs";
// Per-element visibility of the filmstrip's culling badges. Unlike the main
// grid (where these always clamp to the actual photo's visible bounds), the
// filmstrip's thumbnails are small enough that badges may legitimately sit
// outside the letterboxed photo — so instead of clamping, each badge type is
// individually toggleable.
const FILMSTRIP_SHOW_TYPE_KEY = "cullant.filmstrip.showType";
const FILMSTRIP_SHOW_RATING_KEY = "cullant.filmstrip.showRating";
const FILMSTRIP_SHOW_LABEL_KEY = "cullant.filmstrip.showLabel";
const FILMSTRIP_SHOW_FLAG_KEY = "cullant.filmstrip.showFlag";
const FILMSTRIP_SHOW_TAGS_KEY = "cullant.filmstrip.showTags";
const DIM_QUEUED_DELETES_KEY = "cullant.dimQueuedDeletes";
const AUTO_RESCAN_MINUTES_KEY = "cullant.autoRescanMinutes";
const FAST_CULLING_KEY = "cullant.fastCulling";
const LOCK_CAROUSEL_KEY = "cullant.lockCarousel";

/** Allowed auto-rescan intervals in minutes; 0 means off. Kept as a whitelist
 *  so a stale/garbled stored value can never yield a pathological interval. */
export const AUTO_RESCAN_CHOICES = [0, 1, 5, 15] as const;

function loadBool(key: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : JSON.parse(raw) === true;
  } catch {
    return fallback;
  }
}

function loadChoice(key: string, choices: readonly number[], fallback: number): number {
  try {
    const raw = localStorage.getItem(key);
    if (raw === null) return fallback;
    const n = JSON.parse(raw);
    return choices.includes(n) ? n : fallback;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // persistence is best-effort
  }
}

/** The reorderable / hideable groups of the bottom classification bar, in their
 *  default order. Group-level granularity (not individual stars/swatches) mirrors
 *  the bar's own visual grouping. The contextual selection/navigation controls
 *  are not listed here — they appear on their own when relevant. */
export const BOTTOM_BAR_ITEMS = [
  { id: "flags", label: "Pick / Reject" },
  { id: "rating", label: "Star rating" },
  { id: "labels", label: "Color labels" },
  { id: "tags", label: "Tags" },
  { id: "clear", label: "Clear all" },
] as const;

const BOTTOM_BAR_KEY = "cullant.bottomBar";
const BAR_IDS: string[] = BOTTOM_BAR_ITEMS.map((i) => i.id);

/** Load the saved bar layout, healing it against the current item set: unknown
 *  ids are dropped and any known id missing from the saved order is appended
 *  (shown), so a future bar item still appears by default. */
function loadBottomBar(): { order: string[]; hidden: string[] } {
  const fallback = { order: [...BAR_IDS], hidden: [] as string[] };
  try {
    const raw = localStorage.getItem(BOTTOM_BAR_KEY);
    if (raw === null) return fallback;
    const parsed = JSON.parse(raw) as { order?: unknown; hidden?: unknown };
    const savedOrder = Array.isArray(parsed.order)
      ? (parsed.order.filter((x): x is string => typeof x === "string") as string[])
      : [];
    const savedHidden = Array.isArray(parsed.hidden)
      ? (parsed.hidden.filter((x): x is string => typeof x === "string") as string[])
      : [];
    const order = savedOrder.filter((id) => BAR_IDS.includes(id));
    for (const id of BAR_IDS) if (!order.includes(id)) order.push(id);
    const hidden = savedHidden.filter((id) => BAR_IDS.includes(id));
    return { order, hidden };
  } catch {
    return fallback;
  }
}

const initialBottomBar = loadBottomBar();

class SettingsStore {
  /** Paint the cached thumbnail instantly while the sharp preview loads. */
  progressiveLoupe = $state<boolean>(loadBool(PROGRESSIVE_LOUPE_KEY, true));

  /** Remember and restore each project's last sort, media tab, filters and
   *  focused item (persisted in the per-project DB). */
  rememberSession = $state<boolean>(loadBool(REMEMBER_SESSION_KEY, true));

  /** Pregenerate video poster thumbnails. They always run last (after every
   *  photo thumbnail and preview) because ffmpeg extraction is the slow tier;
   *  off skips their background pregeneration entirely. Mirrored to the backend
   *  (see the sync effect in +page.svelte) since the ingest pass reads it. */
  generateVideoThumbs = $state<boolean>(loadBool(GENERATE_VIDEO_THUMBS_KEY, true));

  /** Filmstrip badge visibility (see the keys above for why these exist
   *  separately from the grid, which never lets a badge leave the photo). */
  filmstripShowType = $state<boolean>(loadBool(FILMSTRIP_SHOW_TYPE_KEY, true));
  filmstripShowRating = $state<boolean>(loadBool(FILMSTRIP_SHOW_RATING_KEY, true));
  filmstripShowLabel = $state<boolean>(loadBool(FILMSTRIP_SHOW_LABEL_KEY, true));
  filmstripShowFlag = $state<boolean>(loadBool(FILMSTRIP_SHOW_FLAG_KEY, true));
  filmstripShowTags = $state<boolean>(loadBool(FILMSTRIP_SHOW_TAGS_KEY, true));

  /** Darken the thumbnails of files marked for deletion — reject flag OR
   *  queued delete (grid + filmstrip), so a doomed photo reads at a glance
   *  while culling. The red-X badge stays fully visible — only the photo
   *  itself dims. */
  dimQueuedDeletes = $state<boolean>(loadBool(DIM_QUEUED_DELETES_KEY, true));

  /** How often (minutes) to automatically rescan the open project's folder for
   *  added/removed/changed files; 0 disables it. Only fires while the storage is
   *  reachable, so a disconnected drive isn't polled. A manual rescan is always
   *  available (title-bar menu on desktop, pull-to-refresh on mobile). */
  autoRescanMinutes = $state<number>(loadChoice(AUTO_RESCAN_MINUTES_KEY, AUTO_RESCAN_CHOICES, 5));

  /** Fast culling: in the loupe/compare views, any classification (rating, flag,
   *  label, tag) auto-advances to the next photo — no Caps Lock needed. Off by
   *  default; the grid is never affected. */
  fastCulling = $state<boolean>(loadBool(FAST_CULLING_KEY, false));

  /** Lock the filmstrip to the shown photo: scrolling the strip moves the loupe
   *  to whichever cell is centered (a carousel), instead of scrolling
   *  independently of the selection. Off by default. */
  lockCarousel = $state<boolean>(loadBool(LOCK_CAROUSEL_KEY, false));

  setProgressiveLoupe(on: boolean) {
    this.progressiveLoupe = on;
    save(PROGRESSIVE_LOUPE_KEY, on);
  }

  setRememberSession(on: boolean) {
    this.rememberSession = on;
    save(REMEMBER_SESSION_KEY, on);
  }

  setGenerateVideoThumbs(on: boolean) {
    this.generateVideoThumbs = on;
    save(GENERATE_VIDEO_THUMBS_KEY, on);
  }

  setFilmstripShowType(on: boolean) {
    this.filmstripShowType = on;
    save(FILMSTRIP_SHOW_TYPE_KEY, on);
  }

  setFilmstripShowRating(on: boolean) {
    this.filmstripShowRating = on;
    save(FILMSTRIP_SHOW_RATING_KEY, on);
  }

  setFilmstripShowLabel(on: boolean) {
    this.filmstripShowLabel = on;
    save(FILMSTRIP_SHOW_LABEL_KEY, on);
  }

  setFilmstripShowFlag(on: boolean) {
    this.filmstripShowFlag = on;
    save(FILMSTRIP_SHOW_FLAG_KEY, on);
  }

  setFilmstripShowTags(on: boolean) {
    this.filmstripShowTags = on;
    save(FILMSTRIP_SHOW_TAGS_KEY, on);
  }

  setDimQueuedDeletes(on: boolean) {
    this.dimQueuedDeletes = on;
    save(DIM_QUEUED_DELETES_KEY, on);
  }

  setAutoRescanMinutes(minutes: number) {
    this.autoRescanMinutes = minutes;
    save(AUTO_RESCAN_MINUTES_KEY, minutes);
  }

  setFastCulling(on: boolean) {
    this.fastCulling = on;
    save(FAST_CULLING_KEY, on);
  }

  setLockCarousel(on: boolean) {
    this.lockCarousel = on;
    save(LOCK_CAROUSEL_KEY, on);
  }

  // --- bottom classification bar layout ---
  /** Order the bar's groups appear in (ids from BOTTOM_BAR_ITEMS). */
  bottomBarOrder = $state<string[]>(initialBottomBar.order);
  /** Ids the user has hidden from the bar. */
  bottomBarHidden = $state<string[]>(initialBottomBar.hidden);

  /** Ordered bar items with resolved label + hidden flag — the shape the
   *  settings drag list renders. */
  bottomBarList = $derived(
    this.bottomBarOrder.map((id) => ({
      id,
      label: BOTTOM_BAR_ITEMS.find((m) => m.id === id)?.label ?? id,
      hidden: this.bottomBarHidden.includes(id),
    })),
  );

  private saveBottomBar() {
    save(BOTTOM_BAR_KEY, { order: this.bottomBarOrder, hidden: this.bottomBarHidden });
  }

  moveBottomBarItem(from: number, to: number) {
    const arr = [...this.bottomBarOrder];
    if (from < 0 || from >= arr.length || to < 0 || to >= arr.length || from === to) return;
    const [moved] = arr.splice(from, 1);
    arr.splice(to, 0, moved);
    this.bottomBarOrder = arr;
    this.saveBottomBar();
  }

  toggleBottomBarHidden(id: string) {
    this.bottomBarHidden = this.bottomBarHidden.includes(id)
      ? this.bottomBarHidden.filter((x) => x !== id)
      : [...this.bottomBarHidden, id];
    this.saveBottomBar();
  }

  resetBottomBar() {
    this.bottomBarOrder = [...BAR_IDS];
    this.bottomBarHidden = [];
    this.saveBottomBar();
  }
}

export const settings = new SettingsStore();
