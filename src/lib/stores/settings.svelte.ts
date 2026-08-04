/** App-wide user preferences, persisted in localStorage. */

import type { BurstMode } from "../bursts";
import {
  DEFAULT_RADIAL_SLOTS,
  RADIAL_MOUSE_CHOICES,
  RADIAL_SECTOR_CHOICES,
  healSlots,
  slotKey,
  type RadialMouse,
  type RadialSlot,
} from "../radial";

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
const DIM_DELETES_IN_PREVIEW_KEY = "cullant.dimDeletesInPreview";
const AUTO_RESCAN_MINUTES_KEY = "cullant.autoRescanMinutes";
const FAST_CULLING_KEY = "cullant.fastCulling";
const LOCK_CAROUSEL_KEY = "cullant.lockCarousel";
const SKIP_REJECTED_KEY = "cullant.skipRejected";
const COLLAPSE_BURSTS_KEY = "cullant.collapseBursts";
const BURST_MODE_KEY = "cullant.burstMode";
const BURST_GAP_KEY = "cullant.burstGapSeconds";
const PREVIEW_QUALITY_KEY = "cullant.previewQuality";
const RADIAL_SLOTS_KEY = "cullant.radial.slots";
const RADIAL_SECTORS_KEY = "cullant.radial.sectors";
const RADIAL_MOUSE_KEY = "cullant.radial.mouse";

/** Allowed burst gaps in seconds — a whitelist for the same reason the
 *  auto-rescan intervals are one. */
export const BURST_GAP_CHOICES = [1, 2, 3, 5, 10] as const;

/** Allowed auto-rescan intervals in minutes; 0 means off. Kept as a whitelist
 *  so a stale/garbled stored value can never yield a pathological interval. */
export const AUTO_RESCAN_CHOICES = [0, 1, 5, 15] as const;

/** Loupe preview long edge in pixels. Mirrors PREVIEW_LONG_EDGE_CHOICES in the
 *  backend, which rejects anything else. Beyond sharpness this is a memory
 *  lever: the scaled-decode fast path only engages when the source is at least
 *  twice the target, so 1600 halves a 4000px JPEG's decode where 2560 does not. */
export const PREVIEW_QUALITY_CHOICES = [1600, 2560, 3840] as const;

/** Unchanged from when the size was a constant: nobody gets worse previews
 *  without asking for them. */
export const PREVIEW_QUALITY_DEFAULT = 2560;

export const PREVIEW_QUALITY_LABELS: Record<number, string> = {
  1600: "Balanced",
  2560: "Standard",
  3840: "High",
};

/** Every preference's out-of-the-box value, in one place. The loaders below take
 *  their fallback from here and the settings dialog compares against it to mark
 *  a preference as changed and to reset it. Declared twice, those two readings
 *  would be free to disagree. */
export const DEFAULTS: {
  progressiveLoupe: boolean;
  rememberSession: boolean;
  generateVideoThumbs: boolean;
  previewQuality: number;
  filmstripShowType: boolean;
  filmstripShowRating: boolean;
  filmstripShowLabel: boolean;
  filmstripShowFlag: boolean;
  filmstripShowTags: boolean;
  dimQueuedDeletes: boolean;
  dimDeletesInPreview: boolean;
  autoRescanMinutes: number;
  fastCulling: boolean;
  lockCarousel: boolean;
  skipRejected: boolean;
  collapseBursts: boolean;
  burstMode: BurstMode;
  burstGapSeconds: number;
} = {
  progressiveLoupe: true,
  rememberSession: true,
  generateVideoThumbs: true,
  previewQuality: PREVIEW_QUALITY_DEFAULT,
  filmstripShowType: true,
  filmstripShowRating: true,
  filmstripShowLabel: true,
  filmstripShowFlag: true,
  filmstripShowTags: true,
  dimQueuedDeletes: true,
  dimDeletesInPreview: false,
  autoRescanMinutes: 5,
  fastCulling: false,
  lockCarousel: false,
  skipRejected: false,
  collapseBursts: true,
  burstMode: "fixed",
  burstGapSeconds: 2,
};

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

/** Default number of sectors. Six fits the whole culling vocabulary without any
 *  of them getting thin enough to miss on the move. */
const RADIAL_SECTORS_DEFAULT = 6;

function loadRadialSectors(): number {
  return loadChoice(RADIAL_SECTORS_KEY, RADIAL_SECTOR_CHOICES, RADIAL_SECTORS_DEFAULT);
}

function loadRadialMouse(): RadialMouse {
  try {
    const raw = localStorage.getItem(RADIAL_MOUSE_KEY);
    if (raw === null) return "left";
    const v = JSON.parse(raw);
    return RADIAL_MOUSE_CHOICES.includes(v) ? v : "left";
  } catch {
    return "left";
  }
}

function loadRadialSlots(sectors: number): RadialSlot[] {
  try {
    const raw = localStorage.getItem(RADIAL_SLOTS_KEY);
    return healSlots(raw === null ? null : JSON.parse(raw), sectors);
  } catch {
    return healSlots(null, sectors);
  }
}

const initialRadialSectors = loadRadialSectors();

class SettingsStore {
  /** Paint the cached thumbnail instantly while the sharp preview loads. */
  progressiveLoupe = $state<boolean>(loadBool(PROGRESSIVE_LOUPE_KEY, DEFAULTS.progressiveLoupe));

  /** Remember and restore each project's last sort, media tab, filters and
   *  focused item (persisted in the per-project DB). */
  rememberSession = $state<boolean>(loadBool(REMEMBER_SESSION_KEY, DEFAULTS.rememberSession));

  /** Pregenerate video poster thumbnails. They always run last (after every
   *  photo thumbnail and preview) because frame extraction is the slow tier;
   *  off skips their background pregeneration entirely. Mirrored to the backend
   *  (see the sync effect in +page.svelte) since the ingest pass reads it. */
  generateVideoThumbs = $state<boolean>(loadBool(GENERATE_VIDEO_THUMBS_KEY, DEFAULTS.generateVideoThumbs));
  /** Loupe preview long edge. Changing it invalidates every generated preview,
   *  so the UI confirms first and then discards + regenerates them. */
  previewQuality = $state<number>(
    loadChoice(PREVIEW_QUALITY_KEY, PREVIEW_QUALITY_CHOICES, DEFAULTS.previewQuality),
  );

  /** Filmstrip badge visibility (see the keys above for why these exist
   *  separately from the grid, which never lets a badge leave the photo). */
  filmstripShowType = $state<boolean>(loadBool(FILMSTRIP_SHOW_TYPE_KEY, DEFAULTS.filmstripShowType));
  filmstripShowRating = $state<boolean>(loadBool(FILMSTRIP_SHOW_RATING_KEY, DEFAULTS.filmstripShowRating));
  filmstripShowLabel = $state<boolean>(loadBool(FILMSTRIP_SHOW_LABEL_KEY, DEFAULTS.filmstripShowLabel));
  filmstripShowFlag = $state<boolean>(loadBool(FILMSTRIP_SHOW_FLAG_KEY, DEFAULTS.filmstripShowFlag));
  filmstripShowTags = $state<boolean>(loadBool(FILMSTRIP_SHOW_TAGS_KEY, DEFAULTS.filmstripShowTags));

  /** Darken the thumbnails of files marked for deletion — reject flag OR
   *  queued delete (grid + filmstrip), so a doomed photo reads at a glance
   *  while culling. The red-X badge stays fully visible — only the photo
   *  itself dims. */
  dimQueuedDeletes = $state<boolean>(loadBool(DIM_QUEUED_DELETES_KEY, DEFAULTS.dimQueuedDeletes));

  /** Extend that dimming to the large photo in the loupe and compare views.
   *  Separate from `dimQueuedDeletes` because a dim thumbnail reads as a status
   *  badge while a dim preview is the photo you are judging. Off by default. */
  dimDeletesInPreview = $state<boolean>(loadBool(DIM_DELETES_IN_PREVIEW_KEY, DEFAULTS.dimDeletesInPreview));

  /** How often (minutes) to automatically rescan the open project's folder for
   *  added/removed/changed files; 0 disables it. Only fires while the storage is
   *  reachable, so a disconnected drive isn't polled. A manual rescan is always
   *  available (title-bar menu on desktop, pull-to-refresh on mobile). */
  autoRescanMinutes = $state<number>(loadChoice(AUTO_RESCAN_MINUTES_KEY, AUTO_RESCAN_CHOICES, DEFAULTS.autoRescanMinutes));

  /** Fast culling: in the loupe/compare views, any classification (rating, flag,
   *  label, tag) auto-advances to the next photo — no Caps Lock needed. Off by
   *  default; the grid is never affected. */
  fastCulling = $state<boolean>(loadBool(FAST_CULLING_KEY, DEFAULTS.fastCulling));

  /** Lock the filmstrip to the shown photo: scrolling the strip moves the loupe
   *  to whichever cell is centered (a carousel), instead of scrolling
   *  independently of the selection. Off by default. */
  lockCarousel = $state<boolean>(loadBool(LOCK_CAROUSEL_KEY, DEFAULTS.lockCarousel));

  /** Step over photos already marked for deletion when moving to the next or
   *  previous photo in the loupe and compare views. Only the next/previous
   *  commands skip: picking a thumbnail directly still opens it, however it is
   *  marked. Off by default. */
  skipRejected = $state<boolean>(loadBool(SKIP_REJECTED_KEY, DEFAULTS.skipRejected));

  /** Show each burst as one stacked cell in the grid instead of every frame.
   *  Expanding puts the burst badge on each frame instead. */
  collapseBursts = $state<boolean>(loadBool(COLLAPSE_BURSTS_KEY, DEFAULTS.collapseBursts));

  /** How the burst threshold is chosen. "adaptive" reads the shoot's own
   *  rhythm and falls back to `burstGapSeconds` when the intervals show no
   *  clear split — a sports shoot and a wedding do not photograph alike. */
  burstMode = $state<BurstMode>(
    localStorage.getItem(BURST_MODE_KEY) === '"adaptive"' ? "adaptive" : DEFAULTS.burstMode,
  );
  burstGapSeconds = $state<number>(loadChoice(BURST_GAP_KEY, BURST_GAP_CHOICES, DEFAULTS.burstGapSeconds));

  setBurstMode(mode: BurstMode) {
    this.burstMode = mode;
    save(BURST_MODE_KEY, mode);
  }

  setBurstGapSeconds(seconds: number) {
    this.burstGapSeconds = seconds;
    save(BURST_GAP_KEY, seconds);
  }

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

  /** Persist only. Discarding the previews built for the old size and
   *  regenerating them is the caller's job, after the user has confirmed. */
  setPreviewQuality(longEdge: number) {
    if (!PREVIEW_QUALITY_CHOICES.includes(longEdge as (typeof PREVIEW_QUALITY_CHOICES)[number])) {
      return;
    }
    this.previewQuality = longEdge;
    save(PREVIEW_QUALITY_KEY, longEdge);
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

  setDimDeletesInPreview(on: boolean) {
    this.dimDeletesInPreview = on;
    save(DIM_DELETES_IN_PREVIEW_KEY, on);
  }

  setSkipRejected(on: boolean) {
    this.skipRejected = on;
    save(SKIP_REJECTED_KEY, on);
  }

  setCollapseBursts(on: boolean) {
    this.collapseBursts = on;
    save(COLLAPSE_BURSTS_KEY, on);
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

  // --- radial menu (press and hold over a photo) ---
  /** How many sectors the ring is split into. */
  radialSectors = $state<number>(initialRadialSectors);
  /** What each sector holds, clockwise from the top. Always exactly
   *  `radialSectors` long, and always including the `more` slot. */
  radialSlots = $state<RadialSlot[]>(loadRadialSlots(initialRadialSectors));
  /** Which mouse gesture opens it. Touch always uses press-and-hold. */
  radialMouse = $state<RadialMouse>(loadRadialMouse());

  setRadialSectors(n: number) {
    if (!RADIAL_SECTOR_CHOICES.includes(n as (typeof RADIAL_SECTOR_CHOICES)[number])) return;
    this.radialSectors = n;
    // Resizing the ring re-heals the layout, so growing it fills the new sectors
    // and shrinking it never drops `more`.
    this.radialSlots = healSlots(this.radialSlots.map(slotKey), n);
    save(RADIAL_SECTORS_KEY, n);
    this.saveRadialSlots();
  }

  setRadialMouse(mode: RadialMouse) {
    this.radialMouse = mode;
    save(RADIAL_MOUSE_KEY, mode);
  }

  private saveRadialSlots() {
    save(RADIAL_SLOTS_KEY, this.radialSlots.map(slotKey));
  }

  setRadialSlot(index: number, slot: RadialSlot) {
    if (index < 0 || index >= this.radialSlots.length) return;
    const next = [...this.radialSlots];
    // A slot can only be in one place, so assigning it elsewhere swaps rather
    // than duplicating — two sectors doing the same thing is never intended.
    const existing = next.findIndex((s) => slotKey(s) === slotKey(slot));
    if (existing !== -1) next[existing] = next[index];
    next[index] = slot;
    this.radialSlots = healSlots(next.map(slotKey), this.radialSectors);
    this.saveRadialSlots();
  }

  moveRadialSlot(from: number, to: number) {
    const arr = [...this.radialSlots];
    if (from < 0 || from >= arr.length || to < 0 || to >= arr.length || from === to) return;
    const [moved] = arr.splice(from, 1);
    arr.splice(to, 0, moved);
    this.radialSlots = arr;
    this.saveRadialSlots();
  }

  resetRadial() {
    this.radialSectors = RADIAL_SECTORS_DEFAULT;
    this.radialSlots = healSlots(DEFAULT_RADIAL_SLOTS.map(slotKey), RADIAL_SECTORS_DEFAULT);
    this.radialMouse = "left";
    save(RADIAL_SECTORS_KEY, this.radialSectors);
    save(RADIAL_MOUSE_KEY, this.radialMouse);
    this.saveRadialSlots();
  }
}

export const settings = new SettingsStore();
