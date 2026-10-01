import { listen } from "@tauri-apps/api/event";
import { untrack } from "svelte";
import {
  api,
  rotatedOrientation,
  type CullState,
  type ItemLite,
  type MediaTab,
  type PairScope,
  type SortKey,
  type SyncFrom,
  type Targets,
} from "../api";
import { adaptiveGap, computeBursts } from "../bursts";
import { dayKey, groupCompare, groupDim, usefulGroupDims, type GroupContext } from "../gridGroups";
import {
  apertureBucket,
  focalBucket,
  isoBucket,
  metaTextKey,
  shutterBucket,
} from "../metadataFacets";
import { catalog } from "./catalog.svelte";
import { settings } from "./settings.svelte";
import { tags } from "./tags.svelte";
import { view } from "./view.svelte";
import { folders } from "./folders.svelte";

export type FlagFilter = "all" | "pick" | "reject" | "unflagged" | "anyflag" | "notrejected";

/** What each flag filter keeps, given a photo's flag (1 pick, 0 none, -1 reject).
 *  "all" is handled by the caller, which skips the filter entirely. */
const FLAG_FILTER_TESTS: Record<Exclude<FlagFilter, "all">, (flag: number) => boolean> = {
  pick: (f) => f === 1,
  reject: (f) => f === -1,
  unflagged: (f) => f === 0,
  anyflag: (f) => f !== 0,
  notrejected: (f) => f !== -1,
};
/** Photo file-type composition filter (photos tab only). */
export type TypeFilter = "all" | "raw" | "jpeg" | "rawjpeg";
/** Displayed-aspect orientation filter (applies to photos and videos). */
export type OrientationFilter = "all" | "portrait" | "landscape" | "square";
/** Burst membership filter; inert while the project has no burst. */
export type BurstFilter = "all" | "burst" | "single";

interface RejectionUndo {
  generation: number;
  epoch: number;
  focusId: number | null;
  flags: { id: number; flag: number }[];
  deleteIds: number[];
  compareWithId: number | null;
}

interface SurveyUndo extends Omit<RejectionUndo, "compareWithId"> {
  ids: number[];
  selection: SurveySelection;
}

interface SurveySelection {
  ids: number[];
  anchorId: number | null;
  headId: number | null;
}

interface SurveyReview {
  ids: number[];
  keepIds: number[];
  rejectIds: number[];
  keepFiles: number;
  rejectFiles: number;
  asGroups: boolean;
}

function savedChoice<T>(value: unknown, choices: readonly T[], fallback: T): T {
  return choices.includes(value as T) ? value as T : fallback;
}

function savedString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function savedRating(value: unknown): number {
  return Number.isInteger(value) && Number(value) >= 0 && Number(value) <= 5 ? Number(value) : 0;
}

/** Shape of the per-project session blob persisted in the DB (migration v4).
 *  Every field is optional so blobs written by an older or newer build degrade
 *  gracefully rather than throwing. `focusKey` is a file's relPath (a stable
 *  identity across reopens and rescans). */
interface SavedSession {
  sort?: SortKey;
  sortDesc?: boolean;
  media?: MediaTab;
  gridDensity?: GridDensity;
  groupBy?: string[];
  groupByMedia?: Partial<Record<MediaTab, string[]>>;
  stickyGroupHeader?: boolean;
  filters?: {
    flagFilter?: FlagFilter;
    minRating?: number;
    labelFilter?: string | null;
    tagFilter?: number | null;
    nameFilter?: string;
    typeFilter?: TypeFilter;
    extFilter?: string | null;
    orientationFilter?: OrientationFilter;
    burstFilter?: BurstFilter;
    cameraFilter?: string | null;
    lensFilter?: string | null;
    isoFilter?: string | null;
    apertureFilter?: string | null;
    focalFilter?: string | null;
    shutterFilter?: string | null;
    folderFilter?: string | null;
    folderScope?: Record<string, boolean>;
    dateFilter?: string | null;
  };
  focusKey?: string | null;
}

const SHOW_NAMES_KEY = "cullant.showNames";
const SHOW_FILMSTRIP_KEY = "cullant.showFilmstrip";
const FOLDER_TREE_WIDTH_KEY = "cullant.folderTreeWidth";
const FILMSTRIP_HEIGHT_KEY = "cullant.filmstripHeight";

function loadBoolPref(key: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : JSON.parse(raw) === true;
  } catch {
    return fallback;
  }
}

function loadNumPref(key: string, fallback: number): number {
  try {
    const raw = localStorage.getItem(key);
    const n = raw === null ? NaN : Number(JSON.parse(raw));
    return Number.isFinite(n) ? n : fallback;
  } catch {
    return fallback;
  }
}

function saveNumPref(key: string, value: number) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // persistence is best-effort
  }
}

/** Whether the folder tree should start visible. On a touch device held in
 *  portrait (narrow) the tree would squeeze the grid, so it starts hidden
 *  there; a toolbar/keyboard toggle brings it back. Everywhere else it starts
 *  visible. */
function initialFolderTreeVisible(): boolean {
  try {
    if (typeof window === "undefined") return true;
    const coarse = window.matchMedia?.("(pointer: coarse)").matches ?? false;
    const portrait = window.innerHeight > window.innerWidth;
    return !(coarse && portrait);
  } catch {
    return true;
  }
}

/** Directory portion of a relPath, using forward slashes, "" for the root. */
export function dirOf(relPath: string): string {
  const idx = Math.max(relPath.lastIndexOf("/"), relPath.lastIndexOf("\\"));
  return idx === -1 ? "" : relPath.slice(0, idx).replace(/\\/g, "/");
}

// Keep the existing import path without creating a cycle through this store.
export { LABELS, type Label } from "../labels";

/** Grid thumbnail size. `medium` is the historical default. */
export type GridDensity = "small" | "medium" | "large";

/** One member of a group, as the split RAW+JPG chip needs to draw it. */
export interface PairHalf {
  id: number;
  /** Short display name: "RAW" when the group holds exactly one raw file, else
   *  the extension ("JPG", "ORI"). */
  name: string;
  flag: number;
  rating: number;
  label: string | null;
  queuedDelete: boolean;
}

/** A one-line reading of a pair half, for the chip's tooltip. */
export function describeHalf(h: PairHalf): string {
  const bits: string[] = [];
  if (h.rating > 0) bits.push("★".repeat(h.rating));
  if (h.flag === 1) bits.push("pick");
  else if (h.flag === -1) bits.push("reject");
  if (h.label) bits.push(h.label.toLowerCase());
  if (h.queuedDelete) bits.push("queued for deletion");
  return `${h.name}: ${bits.length > 0 ? bits.join(", ") : "unmarked"}`;
}

class SessionStore {
  flagFilter = $state<FlagFilter>("all");
  minRating = $state(0);
  labelFilter = $state<string | null>(null);
  tagFilter = $state<number | null>(null);
  /** File-name query; "" = no filter. Shared by the search overlay and the
   *  Sort & Filter field, so whichever one the user opens shows what the other
   *  typed. Matched by `nameMatcher` — plain text is a case-insensitive
   *  substring, `/…/` is a regular expression. */
  nameFilter = $state("");
  /** Photo file-type composition filter; inert on the videos tab. */
  typeFilter = $state<TypeFilter>("all");
  /** Single file-extension filter (lowercased, e.g. "cr3"); null = any. In
   *  mirror mode a pair matches when *any* of its members has the extension. */
  extFilter = $state<string | null>(null);
  /** Displayed-aspect orientation filter. */
  orientationFilter = $state<OrientationFilter>("all");
  /** Burst membership filter. Bursts are clustered over groups, so both halves
   *  of a RAW+JPEG pair always answer this filter the same way. */
  burstFilter = $state<BurstFilter>("all");
  /** Photographic-settings filters (photos only; inert on the videos tab).
   *  camera/lens hold an exact EXIF string; iso/aperture/focal hold a bucket
   *  key from `metadataFacets` (a photographic step/range, not a raw value).
   *  null = any. */
  cameraFilter = $state<string | null>(null);
  lensFilter = $state<string | null>(null);
  isoFilter = $state<string | null>(null);
  apertureFilter = $state<string | null>(null);
  focalFilter = $state<string | null>(null);
  shutterFilter = $state<string | null>(null);
  /** UTC capture day, `today`, or `last7days`; null = any. */
  dateFilter = $state<string | null>(null);
  todayDay = $state(new Date().toISOString().slice(0, 10));
  last7DaysStart = $derived(
    new Date(Date.parse(`${this.todayDay}T00:00:00Z`) - 6 * 86400000).toISOString().slice(0, 10),
  );

  matchesDateFilter(day: string, filter = this.dateFilter): boolean {
    if (filter === "today") return day === this.todayDay;
    if (filter === "last7days") return day >= this.last7DaysStart && day <= this.todayDay;
    return filter === null || day === filter;
  }
  /** One specific burst to scope the grid to, by burst key; null = any.
   *  Deliberately NOT persisted: burst keys are derived from the current gap
   *  setting, so a restored one could name a burst this session never forms and
   *  leave the grid mysteriously empty. */
  burstKeyFilter = $state<string | null>(null);
  /** Whether the Filters dropdown panel is open. */
  filtersPanelOpen = $state(false);
  /** Whether the name-search overlay is open (Ctrl+F). */
  searchOpen = $state(false);

  /** True when a Sort & Filter control narrows the grid.
   *  The type filter only counts while on the photos tab (it is inert on
   *  videos), so switching tabs never leaves a phantom "active" badge. */
  hasActiveFilters = $derived.by(
    () =>
      this.flagFilter !== "all" ||
      this.minRating > 0 ||
      this.labelFilter !== null ||
      this.tagFilter !== null ||
      this.nameFilter.trim() !== "" ||
      (this.typeFilter !== "all" && catalog.media === "photos") ||
      this.extFilter !== null ||
      this.orientationFilter !== "all" ||
      this.dateFilter !== null ||
      this.burstKeyFilter !== null ||
      // Same idea for bursts: a project with none offers no such control, so it
      // must not badge the button either.
      (this.burstFilter !== "all" && this.hasBursts) ||
      // The five photographic-settings filters are photo-only, so they never
      // badge the button while the videos tab is active (matching typeFilter).
      this.hasPhotoMetadataFilters,
  );
  private hasPhotoMetadataFilters = $derived(catalog.media === "photos" &&
    [this.cameraFilter, this.lensFilter, this.isoFilter, this.apertureFilter, this.focalFilter, this.shutterFilter].some((filter) => filter !== null));

  /** Clear the filter controls and keep the selected folders. */
  clearFilters() {
    this.flagFilter = "all";
    this.minRating = 0;
    this.labelFilter = null;
    this.tagFilter = null;
    this.nameFilter = "";
    this.typeFilter = "all";
    this.extFilter = null;
    this.orientationFilter = "all";
    this.burstFilter = "all";
    this.cameraFilter = null;
    this.lensFilter = null;
    this.isoFilter = null;
    this.apertureFilter = null;
    this.focalFilter = null;
    this.shutterFilter = null;
    this.dateFilter = null;
    this.burstKeyFilter = null;
    this.clampFocus();
  }

  /** Reset every filter AND the grid view (density/grouping) to their neutral
   *  defaults. SessionStore is a singleton reused across projects, so without
   *  this a filter or grouping choice from the PREVIOUS project would leak into
   *  a new one that doesn't override it (remember-session off, or the new
   *  project has no saved blob yet) — e.g. a lens filter naming a lens the new
   *  project's photos never used would silently empty the grid. Call before
   *  `restoreSessionState()` on every project open; a saved blob then overrides
   *  these defaults with the new project's own remembered choices. */
  resetForNewProject() {
    this.folderFilter = null;
    this.clearFilters();
    this.invalidateRejectionUndo();
    this.clearFocus();
    this.groupByMedia = { photos: [], videos: [] };
    this.gridDensity = "medium";
    this.stickyGroupHeader = false;
    this.shownAlt = {};
    this.expandedBursts = new Set();
    this.gridHiddenIds = new Set();
    this.resetSurvey();
    this.compareWithId = null;
    this.pendingFocusKey = null;
    this.pendingDeleteIds = new Set();
    this.pendingCount = 0;
    this.xmpDirtyCount = 0;
    this.recoupleDialogFor = null;
    this.commitDone = null;
    this.commitDialogOpen = false;
    this.moveDialogOpen = false;
    this.moveDialogTargets = null;
    this.filtersPanelOpen = false;
    this.searchOpen = false;
    this.viewPanelOpen = false;
  }

  /** Single-folder fallback for project sessions saved by older versions. */
  get folderFilter(): string | null {
    return folders.allSelected ? null : Object.keys(folders.scope).find((path) => folders.scope[path]) ?? "";
  }
  set folderFilter(path: string | null) {
    folders.selectOnly(path);
  }
  folderTreeVisible = $state(initialFolderTreeVisible());
  /** Width of the folder-tree panel in px (persisted, drag-resizable). */
  folderTreeWidth = $state<number>(loadNumPref(FOLDER_TREE_WIDTH_KEY, 210));

  setFolderTreeWidth(px: number) {
    this.folderTreeWidth = px;
    saveNumPref(FOLDER_TREE_WIDTH_KEY, px);
  }

  mirrorMode = $state(true);

  /** Index into `filtered`; -1 is the sentinel "no item focused" state (no
   *  grid cell matches, so nothing shows the focus outline). */
  #focusedIndex = $state(-1);
  get focusedIndex() {
    return this.#focusedIndex;
  }
  /** Every focus move goes through here, which is what keeps `stickyFocusId`
   *  honest — the arrow keys assign the index directly and never call
   *  `clampFocus`. */
  set focusedIndex(i: number) {
    const moved = i !== this.#focusedIndex;
    this.#focusedIndex = i;
    const it = this.filtered[i];
    if (it) this.stickyFocusId = it.id;
    // An explicit compare pairing lasts only while the focus stays put: once it
    // walks on, a second pane still showing the old partner would be a mode the
    // user never asked to enter.
    if (moved) this.compareWithId = null;
  }
  /** Id of the item the focus last landed on. An index means nothing once a
   *  filter changes `filtered` underneath it; the id is what lets the focus
   *  return to the same photo instead of to whatever inherited the position.
   *  Not `$state`: it is bookkeeping for `clampFocus`, never rendered. */
  private stickyFocusId: number | null = null;
  /** Column count reported by the grid so ↑/↓ move one visual row. */
  gridCols = $state(1);

  /** Focus key (a file's relPath) awaiting restore from a saved session. It is
   *  applied once a matching item appears in `filtered` — items load
   *  asynchronously after a project opens — and cleared once the user moves
   *  focus themselves (see the restore effect below). */
  pendingFocusKey = $state<string | null>(null);

  /** True while a project is opening and its saved session is being restored.
   *  The persist effect skips writes during this window so the transient
   *  defaults set at open time never clobber the saved blob before it loads. */
  restoring = $state(false);

  /** Multi-selection: file ids of selected items (reassigned on every change). */
  selectedIds = $state<Set<number>>(new Set());
  gridHiddenIds = $state<Set<number>>(new Set());
  /** Index into `filtered` where the last explicit selection started (Shift ranges). */
  selectionAnchor = $state<number | null>(null);
  /** Far end of the last Shift range, so a keyboard extension continues from
   *  where the range ended rather than from the anchor. */
  private rangeHead = $state<number | null>(null);

  /** Grid thumbnail size; persisted per project via the session blob. */
  gridDensity = $state<GridDensity>("medium");
  /** Ordered grouping dimensions (keys from `gridGroups`); [] = no grouping
   *  (the default flat grid). Level 0 is the outermost section. */
  private groupByMedia = $state<Record<MediaTab, string[]>>({ photos: [], videos: [] });
  get groupBy() {
    return this.groupByMedia[catalog.media];
  }
  set groupBy(keys: string[]) {
    this.groupByMedia[catalog.media] = keys.filter(key => {
      const dim = groupDim(key);
      return dim && (!dim.media || dim.media === catalog.media);
    });
  }
  groupDims = $derived.by(() => usefulGroupDims(this.filtered, this.groupContext, this.groupBy, catalog.media));
  activeGroupBy = $derived(catalog.media === "photos" ? this.groupBy :
    this.groupBy.filter(key => this.groupDims.some(dim => dim.key === key)));
  /** Pin the current outermost group's header to the top of the grid while
   *  scrolling (only meaningful when grouping is active). Persisted per project. */
  stickyGroupHeader = $state(false);
  /** Whether the grid-view popover (density + group-by) is open. */
  viewPanelOpen = $state(false);

  setDensity(d: GridDensity) {
    this.gridDensity = d;
  }

  /** Append a grouping level (a dimension not already active). */
  addGroupLevel(key: string) {
    if (!this.groupBy.includes(key)) this.groupBy = [...this.groupBy, key];
  }

  /** Replace the dimension at a level, or remove the level when key is "". A
   *  dimension already used at another level is ignored (no duplicate sections). */
  setGroupLevel(index: number, key: string) {
    if (index < 0 || index >= this.groupBy.length) return;
    if (key === "") {
      this.groupBy = this.groupBy.filter((_, i) => i !== index);
      return;
    }
    if (this.groupBy.includes(key) && this.groupBy[index] !== key) return;
    const next = [...this.groupBy];
    next[index] = key;
    this.groupBy = next;
  }

  removeGroupLevel(index: number) {
    this.groupBy = this.groupBy.filter((_, i) => i !== index);
  }

  /** Move a level from one position to another (drag-reorder the nesting). */
  reorderGroupLevel(from: number, to: number) {
    if (
      from === to ||
      from < 0 ||
      to < 0 ||
      from >= this.groupBy.length ||
      to >= this.groupBy.length
    )
      return;
    const next = [...this.groupBy];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    this.groupBy = next;
  }

  clearGroups() {
    if (this.groupBy.length > 0) this.groupBy = [];
  }

  /** Show filenames under grid thumbnails (persisted). */
  showNames = $state<boolean>(loadBoolPref(SHOW_NAMES_KEY, true));

  toggleShowNames() {
    this.showNames = !this.showNames;
    try {
      localStorage.setItem(SHOW_NAMES_KEY, JSON.stringify(this.showNames));
    } catch {
      // persistence is best-effort
    }
  }

  /** Show the filmstrip/carousel in the loupe and compare views (persisted). */
  showFilmstrip = $state<boolean>(loadBoolPref(SHOW_FILMSTRIP_KEY, true));
  /** Height of the filmstrip/carousel in px (persisted, drag-resizable). */
  filmstripHeight = $state<number>(loadNumPref(FILMSTRIP_HEIGHT_KEY, 104));

  setShowFilmstrip(show: boolean) {
    this.showFilmstrip = show;
    try {
      localStorage.setItem(SHOW_FILMSTRIP_KEY, JSON.stringify(show));
    } catch {
      // persistence is best-effort
    }
  }

  toggleShowFilmstrip() {
    this.setShowFilmstrip(!this.showFilmstrip);
  }

  setFilmstripHeight(px: number) {
    this.filmstripHeight = px;
    saveNumPref(FILMSTRIP_HEIGHT_KEY, px);
  }

  // Auto-advance: Lightroom semantics. Caps Lock ON advances after any
  // classification; Shift inverts the behaviour one-shot.
  autoAdvancePref = $state(false);

  /** groupId -> member id the user flipped to with J (mirror mode only). */
  shownAlt = $state<Record<number, number>>({});
  private itemById = $derived(new Map(catalog.items.map((item) => [item.id, item])));

  /** Fast lookup of a group's members. */
  groupIndex = $derived.by(() => {
    const map = new Map<number, ItemLite[]>();
    for (const item of catalog.items) {
      const members = map.get(item.groupId);
      if (members) members.push(item);
      else map.set(item.groupId, [item]);
    }
    return map;
  });

  /**
   * The members of a group — primary first, then by kind, then by extension —
   * with what each one is actually going to get. null for anything that is not a
   * present multi-file group.
   *
   * Mirror mode shows one row per group, the primary's, so a group whose members
   * disagree renders as whatever the primary says and the rest is invisible.
   * That matters because diverging is a legitimate workflow, not a mistake:
   * queueing the RAWs for deletion and keeping the JPEGs is exactly what
   * `delete.rawOnly` is for, and until now the grid drew no trace of it.
   *
   * `diverged` covers the queue as well as the classification, because "this
   * half is on its way out and that one is not" is the difference the user is
   * most likely to have created on purpose.
   */
  pairHalves(item: ItemLite): { halves: PairHalf[]; diverged: boolean } | null {
    const members = this.groupIndex.get(item.groupId);
    if (!members || members.length < 2) return null;
    // A RAW is called RAW whatever its extension. A shot can hold two of them
    // though (OM System writes ORF+ORI), and two rows both reading "RAW" name
    // nothing — so a collision falls back to the extensions.
    let rawCount = 0;
    for (const m of members) if (m.kind === 0) rawCount++;
    // The backend already decided which file stands for the shot, so the chip
    // leads with that one rather than re-deriving the rule here. A JPG+HEIF
    // group is what breaks without it: both are kind 1, and every HEIF spelling
    // sorts before "jpg", so the member nothing can even decode would head the
    // chip.
    const halves: PairHalf[] = [...members]
      .sort(
        (a, b) =>
          Number(b.isPrimary) - Number(a.isPrimary) ||
          a.kind - b.kind ||
          (a.ext < b.ext ? -1 : a.ext > b.ext ? 1 : 0),
      )
      .map((m) => ({
        id: m.id,
        name: m.kind === 0 && rawCount === 1 ? "RAW" : m.ext.toUpperCase(),
        flag: m.flag,
        rating: m.rating,
        label: m.label,
        queuedDelete: this.pendingDeleteIds.has(m.id),
      }));
    const first = halves[0];
    const diverged = halves.some(
      (h) =>
        h.flag !== first.flag ||
        h.rating !== first.rating ||
        h.label !== first.label ||
        h.queuedDelete !== first.queuedDelete,
    );
    return { halves, diverged };
  }

  /** How `nameFilter` is matched against a file's displayed name (basename plus
   *  extension — the extension is a separate column, so matching the basename
   *  alone meant a query like ".jpg" could never hit anything).
   *
   *  A query wrapped in slashes is a regular expression: `/DSC\d{4}/`, with
   *  optional trailing flags. Anything else is a case-insensitive substring.
   *  `null` means "no filter"; `valid: false` means the pattern does not
   *  compile, which the search overlay reports rather than silently matching
   *  everything.
   */
  nameMatcher = $derived.by<{ test: (name: string) => boolean; valid: boolean } | null>(() => {
    const raw = this.nameFilter.trim();
    if (raw === "") return null;
    const asRegex = /^\/(.+)\/([dgimsuvy]*)$/.exec(raw);
    if (asRegex) {
      // `g` and `y` make `test` stateful through lastIndex, which would make a
      // match depend on how many items were tested before it.
      const flags = asRegex[2].replace(/[gy]/g, "");
      try {
        const rx = new RegExp(asRegex[1], flags.includes("i") ? flags : `${flags}i`);
        return { test: (name: string) => rx.test(name), valid: true };
      } catch {
        return { test: () => false, valid: false };
      }
    }
    const needle = raw.toLowerCase();
    return { test: (name: string) => name.toLowerCase().includes(needle), valid: true };
  });

  /** The gap actually in use, and where it came from. Adaptive detection is
   *  reported rather than hidden: a threshold you cannot see is impossible to
   *  argue with when it guesses wrong. */
  burstGap = $derived.by(() => {
    if (settings.burstMode !== "adaptive") {
      return { seconds: settings.burstGapSeconds, adaptive: false };
    }
    const derived = adaptiveGap(catalog.items);
    return derived === null
      ? { seconds: settings.burstGapSeconds, adaptive: false }
      : { seconds: derived, adaptive: true };
  });

  /** file id -> burst key, for files that belong to a multi-shot burst.
   *  Computed over the whole project rather than the filtered view, so "3 of 8"
   *  keeps meaning 3 of 8 photos taken, not 3 of 8 currently visible. */
  bursts = $derived(computeBursts(catalog.items, this.burstGap.seconds));

  hasBursts = $derived(this.bursts.sizes.size > 0);

  /** What the grouping dimensions need beyond the item itself. */
  groupContext = $derived.by<GroupContext>(() => {
    const burstStarts = new Map<string, number>();
    for (const item of catalog.items) {
      const key = this.bursts.byFile.get(item.id);
      if (!key) continue;
      const time = item.captureTime ?? item.mtime;
      burstStarts.set(key, Math.min(burstStarts.get(key) ?? time, time));
    }
    return { bursts: this.bursts, burstStarts };
  });

  folderScopedItems = $derived.by(() => {
    let out: ItemLite[];
    if (this.mirrorMode) {
      // One entry per logical photo; J may swap which member is displayed.
      out = catalog.items
        .filter((i) => i.isPrimary || i.groupSize <= 1)
        .map((i) => {
          const altId = this.shownAlt[i.groupId];
          if (altId === undefined || altId === i.id) return i;
          const alt = this.groupIndex.get(i.groupId)?.find((m) => m.id === altId);
          return alt ?? i;
        });
    } else {
      out = catalog.items;
    }
    return out.filter((item) => folders.includes(dirOf(item.relPath)) && !folders.ignoredBy(dirOf(item.relPath)));
  });

  filtered = $derived.by(() => {
    let out = this.folderScopedItems;
    if (this.flagFilter !== "all") {
      const keep = FLAG_FILTER_TESTS[this.flagFilter];
      out = out.filter((i) => keep(i.flag));
    }
    if (this.minRating > 0) {
      out = out.filter((i) => i.rating >= this.minRating);
    }
    if (this.labelFilter !== null) {
      out = out.filter((i) => i.label === this.labelFilter);
    }
    if (this.tagFilter !== null) {
      out = out.filter((i) => i.tagIds.includes(this.tagFilter!));
    }
    const matcher = this.nameMatcher;
    if (matcher) {
      out = out.filter((i) => matcher.test(`${i.name}.${i.ext}`));
    }
    // File-type composition (photos only; a video has no RAW/JPEG notion). In
    // mirror mode `out` already holds one entry per group, so groupSize/decoupled
    // read straight off it: a raw+jpeg pair is groupSize > 1 and not decoupled,
    // and a decoupled pair member counts as a lone file of its own kind.
    if (this.typeFilter !== "all" && catalog.media === "photos") {
      out = out.filter((i) => {
        const isPair = i.groupSize > 1 && !i.decoupled;
        switch (this.typeFilter) {
          case "raw":
            return i.kind === 0 && !isPair;
          case "jpeg":
            return i.kind === 1 && !isPair;
          case "rawjpeg":
            return isPair;
          default:
            return true;
        }
      });
    }
    // Single file-extension filter. In mirror mode `out` holds one entry per
    // group, so a pair matches when any of its members carries the extension
    // (a RAW+JPEG pair thus shows under both its RAW and its JPEG extension).
    if (this.extFilter !== null) {
      const want = this.extFilter;
      out = out.filter((i) => {
        if (i.ext.toLowerCase() === want) return true;
        if (!this.mirrorMode) return false;
        const members = this.groupIndex.get(i.groupId);
        return members?.some((m) => m.ext.toLowerCase() === want) ?? false;
      });
    }
    // Orientation by *displayed* aspect. width/height are un-rotated sensor
    // dims; EXIF orientation 5-8 means the shown image is turned 90°, so the
    // displayed dims are swapped. Items with unknown dims are excluded.
    if (this.orientationFilter !== "all") {
      out = out.filter((i) => {
        if (i.width == null || i.height == null) return false;
        const rotated = i.orientation != null && i.orientation >= 5 && i.orientation <= 8;
        const w = rotated ? i.height : i.width;
        const h = rotated ? i.width : i.height;
        switch (this.orientationFilter) {
          case "portrait":
            return h > w;
          case "landscape":
            return w > h;
          case "square":
            return w === h; // exact; sensor dims are integers
          default:
            return true;
        }
      });
    }
    // Burst membership. Inert while the project has no burst, so a filter left
    // over from a shoot that had them cannot silently empty a shoot that has
    // none. `byFile` holds every member id, pairs included, so mirror mode and
    // separate mode agree on what belongs to a burst.
    if (this.burstFilter !== "all" && this.hasBursts) {
      const wantBurst = this.burstFilter === "burst";
      out = out.filter((i) => this.bursts.byFile.has(i.id) === wantBurst);
    }
    if (this.burstKeyFilter !== null) {
      out = out.filter((i) => this.bursts.byFile.get(i.id) === this.burstKeyFilter);
    }
    // Photographic-settings filters. Photo-only (a video carries no camera/lens/
    // ISO/focal/aperture EXIF), so they are inert on the videos tab. camera/lens
    // compare the exact EXIF string; iso/aperture/focal compare the item's bucket
    // key against the picked one, using the very functions the panel built the
    // chips from. In mirror mode `out` holds one entry per pair, but both halves
    // of a RAW+JPEG pair share the same shot, so testing the shown member is exact.
    if (catalog.media === "photos") {
      // Camera and lens are free EXIF text, and the same body can be spelled
      // with different case across bodies or firmware. The panel merges those
      // spellings into one chip, so the match has to fold case on both sides or
      // picking that chip would drop every minority spelling behind it.
      if (this.cameraFilter !== null) {
        const want = metaTextKey(this.cameraFilter);
        out = out.filter((i) => i.camera != null && metaTextKey(i.camera) === want);
      }
      if (this.lensFilter !== null) {
        const want = metaTextKey(this.lensFilter);
        out = out.filter((i) => i.lens != null && metaTextKey(i.lens) === want);
      }
      if (this.isoFilter !== null) {
        out = out.filter((i) => isoBucket(i.iso)?.key === this.isoFilter);
      }
      if (this.apertureFilter !== null) {
        out = out.filter((i) => apertureBucket(i.fNumber)?.key === this.apertureFilter);
      }
      if (this.focalFilter !== null) {
        out = out.filter((i) => focalBucket(i.focalLength)?.key === this.focalFilter);
      }
      if (this.shutterFilter !== null) {
        out = out.filter((i) => shutterBucket(i.exposureTime)?.key === this.shutterFilter);
      }
    }
    if (this.dateFilter !== null) {
      out = out.filter((i) => this.matchesDateFilter(dayKey(i)));
    }
    // Grouping is a stable multi-level sort applied last, so items of the same
    // bucket become contiguous while the active catalog sort survives within the
    // deepest bucket. Keeping it in `filtered` (not just the grid) means the
    // focus/selection index space matches what the grid draws. Copy first: `out`
    // may still alias catalog.items here (separate mode with no active filter).
    if (this.groupBy.length > 0) {
      const ctx = this.groupContext;
      const useful = catalog.media === "videos" ? usefulGroupDims(out, ctx, [], "videos") : [];
      const keys = catalog.media === "videos"
        ? this.groupBy.filter(key => useful.some(dim => dim.key === key))
        : this.groupBy;
      out = [...out].sort((a, b) => groupCompare(a, b, keys, ctx));
    }
    return out;
  });

  /** burst key -> its frames, as indexes into `filtered` and in that order.
   *  Counted over the visible list, so a position reads as "3 of the 8 you can
   *  actually step through" rather than 3 of 8 files on disk. */
  burstRuns = $derived.by(() => {
    const runs = new Map<string, number[]>();
    if (!this.hasBursts) return runs;
    for (let i = 0; i < this.filtered.length; i++) {
      const key = this.bursts.byFile.get(this.filtered[i].id);
      if (key === undefined) continue;
      const run = runs.get(key);
      if (run) run.push(i);
      else runs.set(key, [i]);
    }
    return runs;
  });

  /** Where the photo at a `filtered` index sits in its burst, or null when it
   *  is not in one. Feeds the Layers badge in the grid and the filmstrip. */
  burstPositionAt(index: number): { position: number; total: number } | null {
    const item = this.filtered[index];
    if (!item) return null;
    const key = this.bursts.byFile.get(item.id);
    if (key === undefined) return null;
    const run = this.burstRuns.get(key);
    if (!run || run.length < 2) return null;
    const at = run.indexOf(index);
    return at === -1 ? null : { position: at + 1, total: run.length };
  }

  /** Same, by file id, for the callers that hold a photo rather than its place
   *  in the list. Compare's two panes are the case: either of them can be a
   *  pinned photo that the focus has since walked away from. */
  burstPositionOf(id: number): { position: number; total: number } | null {
    const key = this.bursts.byFile.get(id);
    if (key === undefined) return null;
    const run = this.burstRuns.get(key);
    if (!run || run.length < 2) return null;
    const at = run.findIndex((idx) => this.filtered[idx]?.id === id);
    return at === -1 ? null : { position: at + 1, total: run.length };
  }

  /** Where the focused photo sits in its burst, or null when it is not in one. */
  focusedBurst = $derived.by(() => {
    const item = this.focused;
    if (!item) return null;
    const key = this.bursts.byFile.get(item.id);
    if (key === undefined) return null;
    const members = this.burstRuns.get(key);
    if (!members || members.length < 2) return null;
    const index = members.indexOf(this.focusedIndex);
    if (index === -1) return null;
    return { position: index + 1, total: members.length, members };
  });


  /** Start index (into `filtered`) of every grid cell, when each burst is drawn
   *  as ONE stacked cell. null means one cell per photo, which is both the
   *  common case and the cheap one: no per-photo array is built for it.
   *
   *  Only a run of CONSECUTIVE frames collapses. A sort that scatters a burst
   *  (by name, by size) then yields several stacks instead of one, which is the
   *  honest reading: the grid's order is what the user sees. */
  gridCellStarts = $derived.by<number[] | null>(() => {
    if (!settings.collapseBursts || !this.hasBursts) return null;
    const starts: number[] = [];
    let prev: string | undefined;
    for (let i = 0; i < this.filtered.length; i++) {
      const key = this.bursts.byFile.get(this.filtered[i].id);
      // An individually expanded burst draws a cell per frame, so `prev` is
      // cleared as well: the frame after it must start its own cell too.
      const stacks = key !== undefined && !this.expandedBursts.has(key);
      if (!stacks || key !== prev) starts.push(i);
      prev = stacks ? key : undefined;
    }
    return starts;
  });

  /** Bursts the user opened out while "Collapse bursts" is on, by burst key.
   *  Exceptions to the global setting rather than a second mode: closing the
   *  setting makes them irrelevant without having to be cleared. */
  expandedBursts = $state<Set<string>>(new Set());

  /** The burst key of a `filtered` index, or null when it is in no burst. */
  burstKeyAt(index: number): string | null {
    const item = this.filtered[index];
    return (item && this.bursts.byFile.get(item.id)) ?? null;
  }

  /** Open a collapsed burst out into its frames, or stack it back up. */
  toggleBurstExpanded(key: string) {
    const next = new Set(this.expandedBursts);
    if (!next.delete(key)) next.add(key);
    this.expandedBursts = next;
    this.clampFocus();
  }

  /** How many cells the grid draws. */
  gridCellCount = $derived(this.gridCellStarts?.length ?? this.filtered.length);

  /** First `filtered` index drawn by grid cell `cell`. */
  gridCellStart(cell: number): number {
    const starts = this.gridCellStarts;
    return starts ? starts[cell] : cell;
  }

  /** One past the last `filtered` index drawn by grid cell `cell`. */
  gridCellEnd(cell: number): number {
    const starts = this.gridCellStarts;
    if (!starts) return cell + 1;
    return cell + 1 < starts.length ? starts[cell + 1] : this.filtered.length;
  }

  /** Which grid cell draws a `filtered` index. Cells partition `filtered` in
   *  order, so this is a binary search over the starts. */
  gridCellAt(index: number): number {
    const starts = this.gridCellStarts;
    if (!starts || starts.length === 0) return Math.max(0, index);
    let lo = 0;
    let hi = starts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (starts[mid] <= index) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  }

  /** Every file id the grid cell holding `index` stands for — the whole burst
   *  under a collapsed stack, a single photo otherwise. */
  cellIdsAt(index: number): number[] {
    const item = this.filtered[index];
    if (!item) return [];
    if (view.mode !== "grid" || !this.gridCellStarts) return [item.id];
    const cell = this.gridCellAt(index);
    const out: number[] = [];
    for (let i = this.gridCellStart(cell); i < this.gridCellEnd(cell); i++) {
      out.push(this.filtered[i].id);
    }
    return out;
  }

  /** The starts array, but only where it applies: selection and navigation step
   *  by cell in the grid, by photo everywhere else. */
  private activeCellStarts(): number[] | null {
    return view.mode === "grid" ? this.gridCellStarts : null;
  }

  private visibleNavigationStarts(): number[] | null {
    if (view.mode === "survey") {
      const wanted = new Set(this.surveyItems.map((item) => item.id));
      return this.filtered.flatMap((item, index) => wanted.has(item.id) ? [index] : []);
    }
    if (view.mode !== "grid" || this.gridHiddenIds.size === 0) return null;
    return (this.gridCellStarts ?? this.filtered.map((_, index) => index))
      .filter((index) => !this.gridHiddenIds.has(this.filtered[index].id));
  }

  /** Step within the focused photo's burst (`,` and `.`), stopping at its own
   *  ends rather than walking on into the next one. */
  stepBurst(delta: number) {
    if (view.mode === "survey") return this.moveFocus(delta);
    const burst = this.focusedBurst;
    if (!burst) return;
    const next = burst.position - 1 + delta;
    if (next < 0 || next >= burst.members.length) return;
    this.collapseSelection();
    this.focusedIndex = burst.members[next];
    this.selectionAnchor = this.focusedIndex;
  }


  /** Candidate file ids while the survey is open. It SHRINKS as photos are
   *  eliminated — that is the whole point of the view, and why this is its own
   *  list rather than a filter over `filtered`. */
  surveyIds = $state<number[]>([]);
  surveyRemainingIds = $state<number[]>([]);
  surveyBatch = $state(1);
  surveyBusy = $state(false);
  surveyReview = $state<SurveyReview | null>(null);
  surveyUndo = $state<SurveyUndo | null>(null);
  private surveyEpoch = 0;
  private surveyDetailSelection: SurveySelection | null = null;

  /** The survey's live items, in filtered order. An id that has left the filter
   *  (rejected while "Picks only" is on, say) simply drops out. */
  surveyItems = $derived.by(() => {
    if (this.surveyIds.length === 0) return [];
    const wanted = new Set(this.surveyIds);
    return this.filtered.filter((i) => wanted.has(i.id));
  });

  /** Ceiling on a survey. Every tile loads a full loupe preview, and past this
   *  many they are too small to judge anyway. */
  static readonly MAX_SURVEY = 16;

  /** Total candidates in this Survey, including later batches. */
  surveyRequested = $state(0);
  surveyRemainingCount = $derived.by(() => {
    const remaining = new Set(this.surveyRemainingIds);
    return this.filtered.filter((item) => remaining.has(item.id)).length;
  });
  surveyKeeperIds = $derived(this.surveyItems
    .filter((item) => this.selectedIds.size > 0 ? this.selectedIds.has(item.id) : item.flag === 1)
    .map((item) => item.id));

  /** Candidates for a survey, in filtered order: an explicit selection first,
   *  otherwise the focused photo's burst. Returns [] when there is nothing
   *  worth comparing. */
  private surveyCandidates(): number[] {
    if (this.selectedIds.size >= 2) {
      return this.filtered.filter((i) => this.selectedIds.has(i.id)).map((i) => i.id);
    }
    const burst = this.focusedBurst;
    if (burst) return burst.members.map((idx) => this.filtered[idx].id);
    return [];
  }

  /** True when there is something to survey — drives whether the entry points
   *  offer it at all, so the command is never a silent no-op. */
  canSurvey = $derived(this.surveyCandidates().length >= 2);

  openSurvey() {
    if (view.mode === "survey") return this.leaveSurveyDetail();
    const all = this.surveyCandidates();
    if (all.length < 2) return;
    this.resetSurvey();
    this.surveyRequested = all.length;
    const ids = all.slice(0, SessionStore.MAX_SURVEY);
    this.surveyRemainingIds = all.slice(SessionStore.MAX_SURVEY);
    this.surveyIds = ids;
    const first = this.filtered.findIndex((i) => i.id === ids[0]);
    if (first >= 0) {
      // Dropping the selection is not tidiness: `targets()` prefers a selection
      // over the focus, so leaving it would make every reject hit all N at once
      // — the exact opposite of eliminating one at a time.
      this.collapseSelection();
      this.focusedIndex = first;
      this.selectionAnchor = first;
    }
    view.mode = "survey";
  }

  closeSurvey() {
    this.resetSurvey();
    view.mode = "grid";
  }

  private resetSurvey() {
    this.surveyEpoch++;
    this.surveyIds = [];
    this.surveyRequested = 0;
    this.surveyRemainingIds = [];
    this.surveyBatch = 1;
    this.surveyBusy = false;
    this.surveyReview = null;
    this.surveyUndo = null;
    this.surveyDetailSelection = null;
    view.surveyDetail = false;
  }

  nextSurveyBatch() {
    if (this.surveyBusy) return;
    const remaining = new Set(this.surveyRemainingIds);
    const ids = this.filtered.filter((item) => remaining.has(item.id))
      .slice(0, SessionStore.MAX_SURVEY).map((item) => item.id);
    if (ids.length === 0) return;
    const shown = new Set(ids);
    this.surveyRemainingIds = this.surveyRemainingIds.filter((id) => !shown.has(id));
    this.surveyIds = ids;
    this.surveyBatch++;
    this.surveyUndo = null;
    this.surveyReview = null;
    this.leaveSurveyDetail();
    this.focusSurveyItem(ids[0]);
  }

  inspectSurvey() {
    if (view.surveyDetail) return;
    const id = this.focused?.id;
    if (id === undefined || !this.surveyItems.some((item) => item.id === id)) return;
    this.surveyDetailSelection = this.captureSurveySelection();
    this.collapseSelection();
    view.resetZoom();
    view.surveyDetail = true;
  }

  leaveSurveyDetail(): boolean {
    if (!view.surveyDetail) return false;
    const selection = this.surveyDetailSelection;
    this.surveyDetailSelection = null;
    view.surveyDetail = false;
    view.resetZoom();
    if (selection) this.restoreSurveySelection(selection);
    return true;
  }

  private captureSurveySelection(): SurveySelection {
    return {
      ids: [...this.selectedIds],
      anchorId: this.filtered[this.selectionAnchor ?? -1]?.id ?? null,
      headId: this.filtered[this.rangeHead ?? -1]?.id ?? null,
    };
  }

  private restoreSurveySelection(selection: SurveySelection) {
    if (view.surveyDetail) {
      this.surveyDetailSelection = selection;
      return;
    }
    const visible = new Set(this.surveyItems.map((item) => item.id));
    this.selectedIds = new Set(selection.ids.filter((id) => visible.has(id)));
    this.selectionAnchor = this.surveyIndexOf(selection.anchorId);
    this.rangeHead = this.surveyIndexOf(selection.headId);
  }

  private surveyIndexOf(id: number | null): number | null {
    const index = this.filtered.findIndex((item) => item.id === id);
    return index >= 0 ? index : null;
  }

  /** Move focus to the next/previous survey candidate, stopping at the ends. */
  stepSurvey(delta: number) {
    this.moveFocus(delta);
  }

  /** Focus a survey tile directly (click/tap). */
  focusSurveyItem(id: number) {
    if (!this.surveyItems.some((item) => item.id === id)) return;
    const idx = this.filtered.findIndex((i) => i.id === id);
    if (idx < 0) return;
    this.collapseSelection();
    this.focusedIndex = idx;
    this.selectionAnchor = idx;
  }

  /** Take a photo out of the running. The survey narrows to the remainder and
   *  focus lands on a neighbour, so eliminating is a single keystroke that
   *  leaves you ready for the next one. */
  eliminateFromSurvey(ids: number[]) {
    if (this.surveyIds.length === 0) return;
    const gone = new Set(ids);
    const before = this.surveyItems;
    const at = before.findIndex((i) => gone.has(i.id));
    this.surveyIds = this.surveyIds.filter((id) => !gone.has(id));

    const left = this.surveyItems;
    if (left.length === 0) {
      this.focusedIndex = -1;
      this.clearSelection();
      this.leaveSurveyDetail();
      return;
    }
    // Prefer whatever slid into the vacated slot, else the new last one.
    const target = left[Math.min(at < 0 ? 0 : at, left.length - 1)];
    this.focusSurveyItem(target.id);
  }

  private physicalTargetIds(t: Targets, items = catalog.items): number[] {
    const ids = new Set(t.ids);
    const groups = new Set(items.filter((item) => ids.has(item.id) && !item.decoupled).map((item) => item.groupId));
    return items.filter((item) => ids.has(item.id) || (t.asGroups && !item.decoupled && groups.has(item.groupId)))
      .map((item) => item.id);
  }

  targetFileCount(t: Targets): number {
    return this.physicalTargetIds(t).length;
  }

  reviewSurveyKeepers() {
    const keepIds = this.surveyKeeperIds;
    const kept = new Set(keepIds);
    const ids = this.surveyItems.map((item) => item.id);
    const rejectIds = ids.filter((id) => !kept.has(id));
    if (this.surveyBusy || keepIds.length === 0 || rejectIds.length === 0) return;
    const asGroups = this.mirrorMode;
    this.surveyReview = {
      ids, keepIds, rejectIds, asGroups,
      keepFiles: this.physicalTargetIds({ ids: keepIds, asGroups }).length,
      rejectFiles: this.physicalTargetIds({ ids: rejectIds, asGroups }).length,
    };
  }

  private surveySnapshotCurrent(snapshot: SurveyUndo): boolean {
    return snapshot.generation === catalog.generation && snapshot.epoch === this.surveyEpoch && view.mode === "survey";
  }

  private async captureSurveyUndo(t: Targets): Promise<SurveyUndo | null> {
    const generation = catalog.generation;
    const epoch = this.surveyEpoch;
    const ids = [...this.surveyIds];
    const focusId = this.focused?.id ?? null;
    const selection = this.surveyDetailSelection ?? this.captureSurveySelection();
    const [items, pending] = await Promise.all([
      api.queryItems(catalog.sort, catalog.media, catalog.sortDesc), api.listPending(),
    ]);
    const affected = new Set(this.physicalTargetIds(t, items));
    const snapshot = {
      generation, epoch, ids, focusId, selection,
      flags: items.filter((item) => affected.has(item.id)).map((item) => ({ id: item.id, flag: item.flag })),
      deleteIds: pending.filter((action) => action.action === "delete" && affected.has(action.fileId)).map((action) => action.fileId),
    };
    return this.surveySnapshotCurrent(snapshot) ? snapshot : null;
  }

  private async restoreSurveyFlags(snapshot: SurveyUndo): Promise<boolean> {
    return this.restoreRejectionFlags(snapshot, () => this.surveySnapshotCurrent(snapshot));
  }

  private async restoreRejectionFlags(
    snapshot: Pick<RejectionUndo, "flags" | "deleteIds">,
    isCurrent: () => boolean,
  ): Promise<boolean> {
    for (const flag of [-1, 0, 1]) {
      if (!isCurrent()) return false;
      const ids = snapshot.flags.filter((state) => state.flag === flag).map((state) => state.id);
      if (ids.length === 0) continue;
      const states = await api.setFlag({ ids, asGroups: false }, flag);
      if (!isCurrent()) return false;
      this.applyStates(states);
    }
    const t = { ids: snapshot.flags.map((state) => state.id), asGroups: false };
    if (!isCurrent()) return false;
    await api.removePendingForFiles(t, "delete");
    if (!isCurrent()) return false;
    if (snapshot.deleteIds.length > 0) {
      await api.enqueueAction({ ids: snapshot.deleteIds, asGroups: false }, "delete", null, "both");
    }
    return isCurrent();
  }

  async undoSurveyRejection() {
    const snapshot = this.surveyUndo;
    if (!snapshot || this.surveyBusy || !this.surveySnapshotCurrent(snapshot)) return;
    this.surveyBusy = true;
    try {
      if (!await this.restoreSurveyFlags(snapshot)) return;
      this.surveyIds = snapshot.ids;
      this.surveyUndo = null;
      this.clearSelection();
      this.focusSurveyItem(snapshot.focusId ?? this.surveyItems[0]?.id ?? -1);
      this.restoreSurveySelection(snapshot.selection);
      await this.refreshPending();
    } catch (error) {
      if (this.surveySnapshotCurrent(snapshot)) await this.recoverWrite(error);
    } finally {
      if (snapshot.epoch === this.surveyEpoch) this.surveyBusy = false;
    }
  }

  async applySurveyReview() {
    const review = this.surveyReview;
    if (!review || this.surveyBusy) return;
    this.surveyReview = null;
    const current = this.surveyItems.map((item) => item.id);
    if (review.asGroups !== this.mirrorMode || current.join() !== review.ids.join()) return;
    const epoch = this.surveyEpoch;
    this.surveyBusy = true;
    try {
      const snapshot = await this.captureSurveyUndo({ ids: review.ids, asGroups: review.asGroups });
      if (!snapshot) return;
      this.surveyUndo = snapshot;
      if (!await this.writeSurveyReviewFlags(review, snapshot)) return;
      this.eliminateFromSurvey(review.rejectIds);
      await this.refreshPending();
    } catch (error) {
      if (epoch === this.surveyEpoch) await this.recoverWrite(error);
    } finally {
      if (epoch === this.surveyEpoch) this.surveyBusy = false;
    }
  }

  private async writeSurveyReviewFlags(review: SurveyReview, snapshot: SurveyUndo): Promise<boolean> {
    if (!await this.writeFlag({ ids: review.keepIds, asGroups: review.asGroups }, 1) || !this.surveySnapshotCurrent(snapshot)) return false;
    return await this.writeFlag({ ids: review.rejectIds, asGroups: review.asGroups }, -1) && this.surveySnapshotCurrent(snapshot);
  }

  private async rejectSurvey(t: Targets, event?: KeyboardEvent, override?: Targets) {
    const epoch = this.surveyEpoch;
    this.surveyBusy = true;
    try {
      const snapshot = await this.captureSurveyUndo(t);
      if (!snapshot) return;
      this.advanceFor(t, override, event);
      this.surveyUndo = snapshot;
      if (!await this.writeFlag(t, -1) || !this.surveySnapshotCurrent(snapshot)) return;
      this.eliminateFromSurvey(t.ids);
      await this.refreshPending();
    } catch (error) {
      if (epoch === this.surveyEpoch) await this.recoverWrite(error);
    } finally {
      if (epoch === this.surveyEpoch) this.surveyBusy = false;
    }
  }

  /** Flip which half of the focused pair is displayed (J). */
  togglePairHalf(override?: ItemLite) {
    const item = override ?? this.focused;
    if (!item || item.groupSize < 2) return;
    const members = this.groupIndex.get(item.groupId) ?? [];
    const other = members.find((m) => m.id !== item.id);
    if (!other) return;
    const next = { ...this.shownAlt };
    if (this.shownAlt[item.groupId] === other.id) delete next[item.groupId];
    else next[item.groupId] = other.id;
    this.shownAlt = next;
  }

  focused = $derived<ItemLite | undefined>(this.filtered[this.focusedIndex]);

  counts = $derived.by(() => {
    let pick = 0;
    let reject = 0;
    let unflagged = 0;
    for (const i of this.folderScopedItems) {
      if (i.flag === 1) pick++;
      else if (i.flag === -1) reject++;
      else unflagged++;
    }
    return { pick, reject, unflagged, total: this.folderScopedItems.length };
  });


  /** Serialise the current view + filters + focus for persistence. */
  sessionSnapshot(): SavedSession {
    return {
      sort: catalog.sort,
      sortDesc: catalog.sortDesc,
      media: catalog.media,
      gridDensity: this.gridDensity,
      groupBy: this.groupBy,
      groupByMedia: this.groupByMedia,
      stickyGroupHeader: this.stickyGroupHeader,
      filters: {
        flagFilter: this.flagFilter,
        minRating: this.minRating,
        labelFilter: this.labelFilter,
        tagFilter: this.tagFilter,
        nameFilter: this.nameFilter,
        typeFilter: this.typeFilter,
        extFilter: this.extFilter,
        orientationFilter: this.orientationFilter,
        burstFilter: this.burstFilter,
        cameraFilter: this.cameraFilter,
        lensFilter: this.lensFilter,
        isoFilter: this.isoFilter,
        apertureFilter: this.apertureFilter,
        focalFilter: this.focalFilter,
        shutterFilter: this.shutterFilter,
        folderFilter: this.folderFilter,
        folderScope: folders.scope,
        dateFilter: this.dateFilter,
      },
      focusKey: this.focused?.relPath ?? null,
    };
  }

  /** Load the saved per-project session (if the setting is on) and apply the
   *  sort/media/filters. Focus is deferred to `pendingFocusKey`, applied once a
   *  matching item loads. Best-effort: any error or missing/invalid blob leaves
   *  the defaults chosen at open time untouched. */
  async restoreSessionState() {
    const generation = catalog.generation;
    try {
      if (!settings.rememberSession) return;
      let raw: string | null;
      try {
        raw = await api.getSessionState();
      } catch {
        return;
      }
      if (generation !== catalog.generation) return;
      if (!raw) return;
      let s: SavedSession;
      try {
        s = JSON.parse(raw) as SavedSession;
      } catch {
        return;
      }
      if (!s || typeof s !== "object" || Array.isArray(s)) return;
      this.gridDensity = savedChoice(s.gridDensity, ["small", "medium", "large"] as const, "medium");
      for (const media of ["photos", "videos"] as const) {
        const saved = s.groupByMedia?.[media] ?? (media === (s.media ?? "photos") ? s.groupBy : []);
        if (!Array.isArray(saved)) continue;
        this.groupByMedia[media] = [...new Set(saved.filter(key => {
          const dim = typeof key === "string" ? groupDim(key) : undefined;
          return dim && (!dim.media || dim.media === media);
        }))];
      }
      if (typeof s.stickyGroupHeader === "boolean") this.stickyGroupHeader = s.stickyGroupHeader;
      const f = s.filters;
      if (f) {
        this.flagFilter = savedChoice(f.flagFilter, ["all", "pick", "reject", "unflagged", "anyflag", "notrejected"] as const, "all");
        this.minRating = savedRating(f.minRating);
        this.labelFilter = savedChoice<string | null>(f.labelFilter, ["Red", "Yellow", "Green", "Blue", "Purple"], null);
        this.tagFilter = Number.isSafeInteger(f.tagFilter) && f.tagFilter! > 0 ? f.tagFilter! : null;
        this.nameFilter = savedString(f.nameFilter) ?? "";
        this.typeFilter = savedChoice(f.typeFilter, ["all", "raw", "jpeg", "rawjpeg"] as const, "all");
        this.orientationFilter = savedChoice(f.orientationFilter, ["all", "portrait", "landscape", "square"] as const, "all");
        this.burstFilter = savedChoice(f.burstFilter, ["all", "burst", "single"] as const, "all");
        for (const key of ["extFilter", "cameraFilter", "lensFilter", "isoFilter", "apertureFilter", "focalFilter", "shutterFilter", "folderFilter", "dateFilter"] as const) {
          this[key] = savedString(f[key]);
        }
        folders.restoreScope(f.folderScope);
      }
      this.pendingFocusKey = savedString(s.focusKey);
      // Sort/media re-query the catalog to reorder/reselect the visible items.
      if (s.sort || s.media || typeof s.sortDesc === "boolean") {
        const sort = savedChoice<SortKey | undefined>(s.sort, ["capture", "name", "size"], undefined);
        const media = savedChoice<MediaTab | undefined>(s.media, ["photos", "videos"], undefined);
        await catalog.applyRestoredView(sort, s.sortDesc, media);
      }
      this.clampFocus();
    } finally {
      if (generation === catalog.generation) this.restoring = false;
    }
  }

  clampFocus() {
    if (view.mode === "survey") {
      if (this.selectedIds.size > 0) return;
      const items = this.surveyItems;
      const item = items.find((candidate) => candidate.id === this.stickyFocusId) ?? items[0];
      this.focusedIndex = item ? this.filtered.findIndex((candidate) => candidate.id === item.id) : -1;
      return;
    }
    // Identity first. A filter that hides the focused photo and is then relaxed
    // has to come back to that photo — clamping alone silently moved the focus
    // to whichever item had inherited the index, and a filter matching nothing
    // dropped it to the top of the list.
    if (this.focusedIndex !== -1 && this.stickyFocusId !== null) {
      const at = this.filtered.findIndex((it) => it.id === this.stickyFocusId);
      if (at >= 0) {
        this.focusedIndex = at;
        return;
      }
    }
    const max = Math.max(0, this.filtered.length - 1);
    if (this.focusedIndex > max) this.focusedIndex = max;
    // Preserve a deliberate -1 ("nothing focused"); only pull other
    // out-of-range negatives up into the valid range.
    else if (this.focusedIndex < -1) this.focusedIndex = 0;
    // The setter cannot catch this one: a narrowing filter usually leaves the
    // index alone and swaps the item under it, so nothing is ever assigned.
    // Record what the focus actually landed on — and only from a real item,
    // because while the filter matches nothing there is nothing to record and
    // that is precisely the id that has to survive until it is relaxed.
    const landed = this.filtered[this.focusedIndex];
    if (landed) this.stickyFocusId = landed.id;
  }

  /** Enter the "no item focused" state: no grid cell shows the focus outline,
   *  and any active selection/anchor is dropped. Used on project open and when
   *  switching the Photos/Videos media tab. */
  clearFocus() {
    this.focusedIndex = -1;
    // Drop the remembered photo too. This runs on project open, and an id from
    // the previous project could otherwise collide with a real one here.
    this.stickyFocusId = null;
    this.clearSelection();
  }

  /** Re-keyed whenever a navigation is blocked at the first/last item, so the
   *  active view can play a quick "no more" bounce. `dir` is -1 (start) / +1
   *  (end); `axis` is the movement axis; `n` increments to re-trigger. */
  edgeBump = $state<{ dir: 1 | -1; axis: "x" | "y"; n: number }>({
    dir: 1,
    axis: "x",
    n: 0,
  });

  /** Timestamp of the last emitted edge bump; throttles bursts (fast wheel). */
  private lastEdgeBumpAt = 0;
  /** Minimum gap between edge bumps so a hard scroll produces one clean nudge. */
  private static readonly EDGE_BUMP_COOLDOWN_MS = 250;

  /** Whether a photo is already on its way out: the reject flag, or a queued
   *  delete. The same pair the grid and the filmstrip dim. */
  private markedForDeletion(item: ItemLite): boolean {
    return item.flag === -1 || this.pendingDeleteIds.has(item.id);
  }

  /** Which photo compare puts in its second pane while nothing is pinned: the
   *  next one after the focus, stepping over the ones already marked for
   *  deletion when the setting asks for it. Offering a photo you have already
   *  decided against as the thing to judge against is wasted screen. null when
   *  there is nothing left to pair with. */
  /** A photo picked to fill compare's second pane, instead of the one after the
   *  focus. Set by "Compare with focused"; cleared as soon as the focus moves,
   *  so it is a one-shot pairing and never a mode the user has to undo. */
  compareWithId = $state<number | null>(null);

  compareCompanionIndex = $derived.by<number | null>(() => {
    if (this.compareWithId !== null) {
      const at = this.filtered.findIndex((i) => i.id === this.compareWithId);
      if (at !== -1) return at;
    }
    let i = this.focusedIndex + 1;
    if (settings.skipRejected) {
      while (i < this.filtered.length && this.markedForDeletion(this.filtered[i])) i++;
    }
    return i < this.filtered.length ? i : null;
  });

  /** Whether a classification advances the focus on its own right now, with no
   *  key event to invert it. Read by the views that have to advance something
   *  other than the focus — compare's second pane cannot go through
   *  `maybeAdvance`, which only ever moves the focus. */
  capsLockActive = $state(false);
  autoAdvanceActive = $derived(this.capsLockActive || this.autoAdvancePref || (settings.fastCulling && view.mode !== "grid"));

  /** Step compare's second pane through the filtered order on its own, leaving
   *  the focus (and therefore the other pane) exactly where it is. */
  stepCompanion(delta: number) {
    const at = this.compareCompanionIndex;
    if (at === null) return;
    const next = this.filtered[at + delta];
    if (next) this.compareWithId = next.id;
  }

  /** Show `index` alongside the currently focused photo. */
  compareWith(index: number) {
    const item = this.filtered[index];
    if (!item) return;
    this.ensureFocus();
    if (this.focused?.id === item.id) return;
    this.collapseSelection();
    this.compareWithId = item.id;
    view.mode = "compare";
  }

  /** Where `moveFocus(delta)` lands, or null when it cannot move.
   *  The grid steps by CELL, so one press passes a collapsed burst. The loupe
   *  and compare views step by photo, and step OVER the photos already marked
   *  for deletion when the setting asks for it. */
  private nextFocusIndex(delta: number): number | null {
    const max = this.filtered.length - 1;
    if (max < 0) return null;
    const visible = this.visibleNavigationStarts();
    if (visible) {
      if (visible.length === 0) return null;
      if (this.focusedIndex < 0) return visible[0];
      const at = visible.findIndex((index) => index >= this.focusedIndex);
      const next = Math.min(visible.length - 1, Math.max(0, at + delta));
      return visible[next] === this.focusedIndex ? null : visible[next];
    }
    if (this.focusedIndex < 0) return 0;

    const starts = this.activeCellStarts();
    if (starts) {
      const cell = this.gridCellAt(this.focusedIndex);
      const next = Math.min(starts.length - 1, Math.max(0, cell + delta));
      return next === cell ? null : starts[next];
    }

    let next = Math.min(max, Math.max(0, this.focusedIndex + delta));
    if (settings.skipRejected && (view.mode === "viewer" || view.mode === "compare")) {
      const step = delta >= 0 ? 1 : -1;
      while (next >= 0 && next <= max && this.markedForDeletion(this.filtered[next])) next += step;
      // Everything ahead is marked: hold position rather than wrap or land
      // outside the set.
      if (next < 0 || next > max) return null;
    }
    return next === this.focusedIndex ? null : next;
  }

  moveFocus(delta: number) {
    const next = this.nextFocusIndex(delta);
    if (next === null) {
      if (delta === 0) return;
      // Already at the edge — signal a bounce instead of a silent no-op, but
      // throttle so a fast wheel/swipe burst yields a single clean nudge.
      const now = Date.now();
      if (now - this.lastEdgeBumpAt < SessionStore.EDGE_BUMP_COOLDOWN_MS) return;
      this.lastEdgeBumpAt = now;
      this.edgeBump = {
        dir: delta > 0 ? 1 : -1,
        axis: Math.abs(delta) === 1 ? "x" : "y",
        n: this.edgeBump.n + 1,
      };
      return;
    }
    this.collapseSelection();
    this.focusedIndex = next;
    this.selectionAnchor = this.focusedIndex;
  }

  focusEdge(end: boolean) {
    this.collapseSelection();
    const visible = this.visibleNavigationStarts();
    if (visible) {
      this.focusedIndex = (end ? visible.at(-1) : visible[0]) ?? -1;
      this.selectionAnchor = this.focusedIndex >= 0 ? this.focusedIndex : null;
      return;
    }
    const starts = this.activeCellStarts();
    if (!end || this.filtered.length === 0) this.focusedIndex = 0;
    else if (starts && starts.length > 0) this.focusedIndex = starts[starts.length - 1];
    else this.focusedIndex = this.filtered.length - 1;
    this.selectionAnchor = this.focusedIndex;
  }

  /** Plain navigation drops any selection, so the item you moved onto is what
   *  the next action hits. `targets()` prefers the selection over the focus, so
   *  a selection left behind would silently send P/X/1-5 to the photos you
   *  navigated away from — and the grid hides the focus ring while a selection
   *  exists, so there would be no visible cue either. */
  private collapseSelection() {
    if (this.selectedIds.size > 0) this.selectedIds = new Set();
    this.rangeHead = null;
  }

  /** Entering the loupe/compare view with nothing focused would otherwise show
   *  an empty pane — default to the first item (compare then naturally shows
   *  the first two: `right` derives as `focusedIndex + 1` when unpinned). A
   *  no-op when something is already focused. */
  ensureFocus() {
    if (view.mode === "survey") return this.clampFocus();
    if (this.focusedIndex === -1 && this.filtered.length > 0) {
      this.focusedIndex = this.visibleNavigationStarts()?.[0] ?? (this.gridHiddenIds.size > 0 ? -1 : 0);
      this.selectionAnchor = this.focusedIndex >= 0 ? this.focusedIndex : null;
    }
  }


  /** Plain click: focus only, drop any selection. */
  selectOnly(index: number) {
    if (view.mode === "survey" && !this.surveyItems.some((item) => item.id === this.filtered[index]?.id)) return;
    this.focusedIndex = index;
    this.selectionAnchor = index;
    if (this.selectedIds.size > 0) this.selectedIds = new Set();
  }

  /** Select an explicit set of files (a whole burst, every shot from one camera).
   *  Drops the focus for the same reason the marquee does: `targets()` prefers a
   *  selection, so a surviving focus outline would claim a cell the next action
   *  is not going to hit. */
  selectIds(ids: Iterable<number>) {
    const allowed = new Set(this.selectionItems().map((item) => item.id));
    const next = new Set([...ids].filter((id) => allowed.has(id)));
    if (next.size === 0) return;
    this.selectedIds = next;
    this.focusedIndex = -1;
  }

  /** Ctrl+click: toggle one cell; an empty selection is seeded from focus. A
   *  collapsed burst toggles as a block — every frame it hides goes with it. */
  toggleSelect(index: number) {
    const item = this.filtered[index];
    if (!item || !this.selectionItems().some((candidate) => candidate.id === item.id)) return;
    const ids = this.cellIdsAt(index);
    const wasEmpty = this.selectedIds.size === 0;
    const next = new Set(this.selectedIds);
    if (wasEmpty && this.focused) for (const id of this.cellIdsAt(this.focusedIndex)) next.add(id);
    // Ctrl+clicking the focused item of an empty selection selects it —
    // seed + toggle would cancel out, so skip the toggle in that one case.
    if (!(wasEmpty && this.focused?.id === item.id)) {
      if (ids.every((id) => next.has(id))) for (const id of ids) next.delete(id);
      else for (const id of ids) next.add(id);
    }
    this.selectedIds = next;
    // Making a selection drops the focus outline entirely: a focused cell next
    // to (or on) the selection reads as "still active", so on touch a tap that
    // deselects a cell but re-focuses it looks like nothing happened. Anchor is
    // kept for a subsequent Shift-range.
    this.focusedIndex = -1;
    this.selectionAnchor = index;
    this.rangeHead = index;
  }

  /** Select the contiguous run between two indexes. `additive` keeps what was
   *  already selected (Ctrl held). */
  private fillRange(anchor: number, head: number, additive = false) {
    let lo = Math.min(anchor, head);
    let hi = Math.max(anchor, head);
    const starts = this.activeCellStarts();
    if (starts) {
      // A range that touches a collapsed burst takes all of it: the grid draws
      // one cell there, so there is no way to point at a single hidden frame.
      lo = this.gridCellStart(this.gridCellAt(lo));
      hi = this.gridCellEnd(this.gridCellAt(hi)) - 1;
    }
    const next = additive ? new Set(this.selectedIds) : new Set<number>();
    const allowed = new Set(this.selectionItems().map((item) => item.id));
    for (let i = lo; i <= hi; i++) {
      const item = this.filtered[i];
      if (item && allowed.has(item.id)) next.add(item.id);
    }
    this.selectedIds = next;
  }

  /** Shift+click: contiguous range from the anchor. `additive` = Ctrl held. */
  rangeSelect(index: number, additive = false) {
    if (!this.selectionItems().some((item) => item.id === this.filtered[index]?.id)) return;
    const anchor = this.selectionAnchor ?? this.focusedIndex;
    this.fillRange(anchor, index, additive);
    // Drop focus while a selection exists (see toggleSelect); keep the anchor.
    this.focusedIndex = -1;
    this.selectionAnchor = anchor;
    this.rangeHead = index;
  }

  /** Index the next Shift+Arrow grows from. Focus is the head while it is live;
   *  once a selection has dropped the focus outline, `rangeHead` remembers where
   *  the range ended so extending continues from there instead of collapsing
   *  back to the anchor. */
  private selectionHeadIndex(): number {
    if (this.focusedIndex >= 0) return this.focusedIndex;
    return this.rangeHead ?? this.selectionAnchor ?? 0;
  }

  /** Shift+Arrow: move the head by `delta` and reselect from the anchor, so the
   *  range grows and shrinks as the head passes back over the anchor. Focus
   *  tracks the head — the grid only draws the focus ring when nothing is
   *  selected, so no stray outline appears, and the existing scroll-into-view
   *  effect keeps the head on screen for free. */
  extendSelection(delta: number) {
    if (this.filtered.length === 0) return;
    const head = this.selectionHeadIndex();
    const visible = this.visibleNavigationStarts();
    if (visible) {
      const at = visible.findIndex((index) => index >= head);
      const next = visible[Math.min(visible.length - 1, Math.max(0, at + delta))];
      if (next !== undefined && next !== head) this.extendTo(next);
      return;
    }
    const starts = this.activeCellStarts();
    if (starts) {
      const cell = this.gridCellAt(head);
      const nextCell = Math.min(starts.length - 1, Math.max(0, cell + delta));
      if (nextCell === cell) return;
      this.extendTo(starts[nextCell]);
      return;
    }
    const max = this.filtered.length - 1;
    const next = Math.min(max, Math.max(0, head + delta));
    if (next === head) return;
    this.extendTo(next);
  }

  /** Shift+Home / Shift+End: extend the selection to the first/last item. */
  extendToEdge(end: boolean) {
    if (this.filtered.length === 0) return;
    const visible = this.visibleNavigationStarts();
    if (visible) {
      const next = end ? visible.at(-1) : visible[0];
      if (next !== undefined) this.extendTo(next);
      return;
    }
    this.extendTo(end ? this.filtered.length - 1 : 0);
  }

  private extendTo(head: number) {
    const anchor = this.selectionAnchor ?? this.selectionHeadIndex();
    this.selectionAnchor = anchor;
    this.fillRange(anchor, head);
    this.focusedIndex = head;
    this.rangeHead = head;
  }

  clearSelection() {
    if (this.selectedIds.size > 0) this.selectedIds = new Set();
    this.selectionAnchor = null;
    this.rangeHead = null;
  }

  selectAll() {
    this.selectedIds = new Set(this.selectionItems().map((item) => item.id));
    // No focus while a selection exists (see toggleSelect).
    this.focusedIndex = -1;
  }

  /** Select everything the current selection leaves out. */
  invertSelection() {
    const next = new Set<number>();
    for (const item of this.selectionItems()) {
      if (!this.selectedIds.has(item.id)) next.add(item.id);
    }
    this.selectedIds = next;
    if (next.size > 0) this.focusedIndex = -1;
  }

  /**
   * Toggle mirror/separate while keeping the same photo under focus. Flipping
   * the mode changes what `filtered` contains (pairs collapse/expand), so the
   * bare index would point at a different photo — re-find it by file id, then
   * by group, then clamp. Selection ids also change meaning, so drop them.
   */
  setMirrorMode(on: boolean) {
    if (on === this.mirrorMode) return;
    const current = this.focused;
    this.mirrorMode = on;
    this.clearSelection();
    if (!current) {
      this.clampFocus();
      return;
    }
    const next = this.filtered;
    let idx = next.findIndex((i) => i.id === current.id);
    if (idx < 0) idx = next.findIndex((i) => i.groupId === current.groupId);
    this.focusedIndex = idx >= 0 ? idx : Math.min(this.focusedIndex, next.length - 1);
    this.clampFocus();
  }

  private selectionItems(): ItemLite[] {
    return view.mode === "survey" ? this.surveyItems : this.filtered.filter((item) => !this.gridHiddenIds.has(item.id));
  }

  private directTargetIds(): number[] {
    return this.selectedIds.size > 0 ? [...this.selectedIds] : this.focused ? [this.focused.id] : [];
  }

  private surveyTargets(override?: Targets): Targets | null {
    if (this.surveyBusy || this.surveyReview) return null;
    const items = view.surveyDetail ? this.surveyItems.filter((item) => item.id === this.focused?.id) : this.surveyItems;
    const allowed = new Set(items.map((item) => item.id));
    const wanted = override?.ids ?? (view.surveyDetail ? items.map((item) => item.id) : this.directTargetIds());
    const ids = wanted.filter((id) => allowed.has(id));
    return ids.length > 0 ? { ids, asGroups: override?.asGroups ?? this.mirrorMode } : null;
  }

  private invalidateSurveyUndo(t: Targets) {
    const affected = new Set(this.physicalTargetIds(t));
    if (this.surveyUndo?.flags.some((state) => affected.has(state.id))) this.surveyUndo = null;
    if (this.lastRejection?.flags.some((state) => affected.has(state.id))) this.invalidateRejectionUndo();
  }

  private lastRejection = $state<RejectionUndo | null>(null);
  private rejectionBusy = $state(false);
  private rejectionEpoch = 0;

  invalidateRejectionUndo() {
    this.rejectionEpoch++;
    this.lastRejection = null;
    this.rejectionBusy = false;
    this.surveyUndo = null;
  }

  private rejectionSnapshotCurrent(snapshot: RejectionUndo): boolean {
    return snapshot.generation === catalog.generation && snapshot.epoch === this.rejectionEpoch;
  }

  private async captureRejectionUndo(t: Targets): Promise<RejectionUndo | null> {
    const generation = catalog.generation;
    const epoch = this.rejectionEpoch;
    const focusId = this.focused?.id ?? null;
    const compareWithId = this.compareWithId;
    const [items, pending] = await Promise.all([
      api.queryItems(catalog.sort, catalog.media, catalog.sortDesc), api.listPending(),
    ]);
    const affected = new Set(this.physicalTargetIds(t, items));
    const snapshot = {
      generation, epoch, focusId, compareWithId,
      flags: items.filter((item) => affected.has(item.id)).map((item) => ({ id: item.id, flag: item.flag })),
      deleteIds: pending.filter((action) => action.action === "delete" && affected.has(action.fileId)).map((action) => action.fileId),
    };
    return this.rejectionSnapshotCurrent(snapshot) ? snapshot : null;
  }

  async undoLastRejection() {
    const snapshot = this.lastRejection;
    if (!snapshot || !this.canUndoRejection) return;
    this.rejectionBusy = true;
    try {
      if (!await this.restoreRejectionFlags(snapshot, () => this.rejectionSnapshotCurrent(snapshot))) return;
      this.lastRejection = null;
      const index = this.filtered.findIndex((item) => item.id === snapshot.focusId);
      if (index >= 0) {
        this.clearSelection();
        this.focusedIndex = index;
        this.compareWithId = snapshot.compareWithId;
      }
      await this.refreshPending();
    } catch (error) {
      if (this.rejectionSnapshotCurrent(snapshot)) await this.recoverWrite(error);
    } finally {
      if (this.rejectionSnapshotCurrent(snapshot)) this.rejectionBusy = false;
    }
  }

  private async rejectWithRecovery(t: Targets, event?: KeyboardEvent, override?: Targets) {
    const epoch = ++this.rejectionEpoch;
    this.lastRejection = null;
    this.rejectionBusy = true;
    try {
      const snapshot = await this.captureRejectionUndo(t);
      if (!snapshot) return;
      this.advanceFor(t, override, event);
      if (!await this.writeFlag(t, -1, () => this.rejectionSnapshotCurrent(snapshot))) return;
      this.lastRejection = snapshot;
      await this.refreshPending();
    } catch (error) {
      if (epoch === this.rejectionEpoch) await this.recoverWrite(error);
    } finally {
      if (epoch === this.rejectionEpoch) this.rejectionBusy = false;
    }
  }

  targets(override?: Targets): Targets | null {
    if (view.mode === "survey") return this.surveyTargets(override);
    if (override) return override;
    if (this.selectedIds.size > 0) {
      const ids = [...this.selectedIds].filter((id) => !this.gridHiddenIds.has(id));
      return ids.length > 0 ? { ids, asGroups: this.mirrorMode } : null;
    }
    const item = this.focused;
    if (!item || (view.mode === "grid" && this.gridHiddenIds.has(item.id))) return null;
    // A collapsed burst cell stands for every frame under it, so a rating, a
    // flag or a tag applied to the stack lands on the whole burst.
    const ids = this.cellIdsAt(this.focusedIndex);
    return { ids: ids.length > 0 ? ids : [item.id], asGroups: this.mirrorMode };
  }

  /// Merge authoritative rows back into the catalog (optimistic UI included:
  /// the same merge applies local guesses and server truth).
  applyStates(states: CullState[]) {
    if (states.length === 0) return;
    const byId = new Map(states.map((s) => [s.id, s]));
    for (const item of catalog.items) {
      const s = byId.get(item.id);
      if (s) {
        item.rating = s.rating;
        item.flag = s.flag;
        item.label = s.label;
        item.orientation = s.orientation;
      }
    }
    this.clampFocus();
  }

  private async classify(t: Targets, patch: Partial<CullState>, run: () => Promise<CullState[]>): Promise<boolean> {
    const generation = catalog.generation;
    const previous = t.ids.map((id) => this.localGuess(id, {}));
    this.applyStates(t.ids.map((id) => this.localGuess(id, patch)));
    try {
      const states = await run();
      if (generation !== catalog.generation) return false;
      this.applyStates(states);
      return true;
    } catch (error) {
      if (generation !== catalog.generation) return false;
      this.applyStates(previous);
      await this.recoverWrite(error);
      return false;
    }
  }

  private async recoverWrite(error: unknown) {
    const generation = catalog.generation;
    try {
      await catalog.refresh();
    } catch {
      // Keep the write error when recovery also fails.
    }
    if (generation === catalog.generation) catalog.error = String(error);
  }

  private maybeAdvance(event?: KeyboardEvent) {
    const caps = event?.getModifierState("CapsLock") ?? false;
    const shift = event?.shiftKey ?? false;
    // Fast culling: in the loupe/compare views every classification jumps to the
    // next photo, no Caps Lock needed. Folds into the same XOR, so Shift can
    // still hold position for a one-off. The grid is never affected.
    const fast = settings.fastCulling && view.mode !== "grid";
    const advance = (caps || this.autoAdvancePref || fast) !== shift; // XOR: shift inverts
    if (advance) this.moveFocus(1);
  }

  /** Auto-advance, unless an explicit target says otherwise.
   *
   *  The radial menu acts on the pane it was opened over, which in compare need
   *  not be the focused one. Advancing then would move a pane the user is not
   *  even looking at, so an explicit target only advances when it IS the focus. */
  private advanceFor(t: Targets, override: Targets | undefined, event?: KeyboardEvent) {
    if (override) {
      const id = this.focused?.id;
      if (id === undefined || !t.ids.includes(id)) return;
    }
    this.maybeAdvance(event);
  }

  async rate(rating: number, event?: KeyboardEvent, override?: Targets) {
    const t = this.targets(override);
    if (!t) return;
    this.advanceFor(t, override, event);
    await this.classify(t, { rating }, () => api.setRating(t, rating));
  }

  async flag(flag: number, event?: KeyboardEvent, override?: Targets) {
    const t = this.targets(override);
    if (!t || this.rejectionBusy) return;
    if (flag === -1 && view.mode === "survey") return this.rejectSurvey(t, event, override);
    this.invalidateSurveyUndo(t);
    if (flag === -1 && view.mode !== "grid") return this.rejectWithRecovery(t, event, override);
    this.advanceFor(t, override, event);
    if (!await this.writeFlag(t, flag)) return;
    await this.refreshPending();
  }

  private async writeFlag(t: Targets, flag: number, isCurrent: () => boolean = () => true): Promise<boolean> {
    const generation = catalog.generation;
    const current = () => generation === catalog.generation && isCurrent();
    if (!current() || !await this.classify(t, { flag }, () => api.setFlag(t, flag)) || !current()) return false;
    // Invariant: reject flag = queued delete. Every flag write funnels through
    // here, so this is the single place that keeps the two in sync: rejecting
    // enqueues a delete, un-rejecting/picking removes it. Same Targets as the
    // flag write, so the backend pair fan-out matches.
    try {
      if (flag === -1) await api.enqueueAction(t, "delete", null, "both");
      else await api.removePendingForFiles(t, "delete");
    } catch (error) {
      if (generation === catalog.generation) await this.recoverWrite(error);
      return false;
    }
    return current();
  }

  async toggleFlag(event?: KeyboardEvent) {
    const item = this.focused;
    if (!item) return;
    await this.flag(item.flag === 1 ? 0 : 1, event);
  }

  async label(label: string | null, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    const item = this.focused;
    // Pressing the same label key again clears it (Lightroom behaviour).
    const next = item && item.label === label ? null : label;
    this.maybeAdvance(event);
    await this.classify(t, { label: next }, () => api.setLabel(t, next));
  }

  /** Apply a label (null clears) exactly as given — no toggle. The bars use
   *  this: they already resolve set-vs-clear from the selection-uniform state
   *  they display, unlike the keyboard path (`label`), which toggles off the
   *  focused item Lightroom-style. */
  async setLabel(label: string | null, override?: Targets) {
    const t = this.targets(override);
    if (!t) return;
    // Fast culling advances on any classification from the bars too, matching the
    // keyboard `label()` path. No-ops outside fast culling / outside loupe-compare.
    this.advanceFor(t, override);
    await this.classify(t, { label }, () => api.setLabel(t, label));
  }

  /** Wipe ALL classification off the current targets in one action: rating,
   *  flag, color label and every task tag. Clearing the flag also unqueues the
   *  delete a reject implies (the same reject-flag = queued-delete invariant the
   *  `flag()` choke point maintains). Never auto-advances — a reset is not a
   *  cull step. */
  async clearClassification() {
    const t = this.targets();
    if (!t || this.rejectionBusy) return;
    this.invalidateSurveyUndo(t);
    const generation = catalog.generation;
    if (!await this.classify(t, { rating: 0 }, () => api.setRating(t, 0))) return;
    if (!await this.classify(t, { flag: 0 }, () => api.setFlag(t, 0))) return;
    if (!await this.classify(t, { label: null }, () => api.setLabel(t, null))) return;
    try {
      await api.removePendingForFiles(t, "delete");
      if (generation !== catalog.generation) return;
      const changes = await api.clearTaskTags(t);
      if (generation !== catalog.generation) return;
      tags.applyChanges(changes);
    } catch (error) {
      if (generation === catalog.generation) await this.recoverWrite(error);
      return;
    }
    await this.refreshPending();
  }

  /** Turn the current targets a quarter turn; positive `steps` is clockwise.
   *  The new orientation comes back from the backend instead of being guessed
   *  locally, so the EXIF rotation table lives in exactly one place. */
  async rotate(steps: number, override?: Targets) {
    const t = this.targets(override);
    if (!t) return;
    const generation = catalog.generation;
    const previous = t.ids.map((id) => this.localGuess(id, {}));
    this.applyStates(
      t.ids.map((id) => {
        const current = this.itemById.get(id);
        return this.localGuess(id, {
          orientation: rotatedOrientation(current?.orientation ?? null, steps),
        });
      }),
    );
    try {
      const states = await api.rotate(t, steps);
      if (generation === catalog.generation) this.applyStates(states);
    } catch (error) {
      if (generation !== catalog.generation) return;
      this.applyStates(previous);
      await this.recoverWrite(error);
    }
  }

  /** Toggle a task tag on the focused photo (fan-out included). */
  async toggleTag(tagId: number, event?: KeyboardEvent, override?: Targets) {
    const t = this.targets(override);
    if (!t) return;
    const generation = catalog.generation;
    this.advanceFor(t, override, event);
    try {
      const changes = await api.toggleTaskTag(t, tagId);
      if (generation !== catalog.generation) return;
      tags.applyChanges(changes);
    } catch (error) {
      if (generation === catalog.generation) await this.recoverWrite(error);
    }
  }

  /** File ids with a queued delete (for grid badges). */
  pendingDeleteIds = $state<Set<number>>(new Set());
  pendingCount = $state(0);
  /** Photos with a pending XMP sidecar write. Tracked separately because XMP
   *  dirtiness is a per-file flag, not a pending-actions row — a plain rating or
   *  label edit leaves nothing in the queue yet still needs a commit. */
  xmpDirtyCount = $state(0);
  commitDialogOpen = $state(false);
  canUndoRejection = $derived(this.lastRejection !== null && !this.rejectionBusy &&
    this.lastRejection.generation === catalog.generation && !this.commitDialogOpen);
  moveDialogOpen = $state(false);
  moveDialogTargets = $state<Targets | null>(null);
  /** Result of the most recent commit, surfaced as a popup AFTER the commit
   *  dialog closes (null = nothing to announce). */
  commitDone = $state<{ title: string; message: string } | null>(null);

  /** Whether the commit button should light up: anything queued OR any pending
   *  XMP write. This is the full set of work a commit would perform. */
  hasCommitWork = $derived(this.pendingCount > 0 || this.xmpDirtyCount > 0);

  async refreshPending() {
    const generation = catalog.generation;
    const request = ++this.pendingRequest;
    const [pending, xmpDirty] = await Promise.all([api.listPending(), api.xmpDirtyCount()]);
    if (generation !== catalog.generation || request !== this.pendingRequest) return;
    this.pendingCount = pending.length;
    this.xmpDirtyCount = xmpDirty;
    this.pendingDeleteIds = new Set(
      pending.filter((p) => p.action === "delete").map((p) => p.fileId)
    );
  }
  private pendingRequest = 0;

  /** Reconcile rejects made before the reject-flag/delete-queue sync existed:
   *  enqueue a delete for every file in the catalog still at flag -1. Runs once
   *  per project open; enqueueAction upserts, so it's idempotent. Queries both
   *  media tabs (the full catalog) rather than the currently loaded one. */
  async syncRejectedToQueue() {
    const generation = catalog.generation;
    const ids: number[] = [];
    // The active tab is already loaded, and this runs during the open, when the
    // storage and the DB are at their busiest — so read the flags off what is
    // in hand rather than pulling the whole catalogue back over IPC.
    for (const i of catalog.items) if (i.flag === -1) ids.push(i.id);
    const other = catalog.media === "photos" ? "videos" : "photos";
    if (catalog.mediaCounts[other] > 0) {
      const items = await api.queryItems("capture", other, false);
      if (generation !== catalog.generation) return;
      for (const i of items) if (i.flag === -1) ids.push(i.id);
    }
    if (ids.length === 0) return;
    await api.enqueueAction({ ids, asGroups: false }, "delete", null, "both");
    if (generation !== catalog.generation) return;
    await this.refreshPending();
  }

  /** Queue a delete for the focused photo. Scope picks pair members. */
  async queueDelete(scope: PairScope, event?: KeyboardEvent, override?: Targets) {
    const t = this.targets(override);
    if (!t || this.rejectionBusy) return;
    this.invalidateSurveyUndo(t);
    const generation = catalog.generation;
    try {
      await api.enqueueAction(t, "delete", null, scope);
    } catch (error) {
      if (generation === catalog.generation) await this.recoverWrite(error);
      return;
    }
    if (generation !== catalog.generation) return;
    this.advanceFor(t, override, event);
    await this.refreshPending();
  }

  /** Group id awaiting a recouple sync choice (renders PairSyncDialog). */
  recoupleDialogFor = $state<number | null>(null);

  /** Ctrl+J: decouple a linked pair, or start recoupling a split one. */
  async togglePairCoupling(override?: ItemLite) {
    const item = override ?? this.focused;
    if (!item || item.groupSize < 2) return;
    if (item.decoupled) {
      this.recoupleDialogFor = item.groupId;
    } else {
      await api.decoupleGroup(item.groupId);
      await catalog.refresh();
    }
  }

  async recouple(groupId: number, syncFrom: SyncFrom) {
    this.recoupleDialogFor = null;
    await api.recoupleGroup(groupId, syncFrom);
    await catalog.refresh();
  }

  /** Settle a coupled pair whose halves have drifted apart. Returns the
   *  authoritative rows, so this reconciles like any other state write and needs
   *  no catalog refresh. */
  async syncPair(groupId: number, syncFrom: SyncFrom) {
    const generation = catalog.generation;
    try {
      const states = await api.syncPairState(groupId, syncFrom);
      if (generation === catalog.generation) this.applyStates(states);
    } catch (error) {
      if (generation === catalog.generation) await this.recoverWrite(error);
    }
  }

  private localGuess(id: number, patch: Partial<CullState>): CullState {
    const current = this.itemById.get(id);
    return {
      id,
      rating: patch.rating ?? current?.rating ?? 0,
      flag: patch.flag ?? current?.flag ?? 0,
      label: "label" in patch ? (patch.label ?? null) : (current?.label ?? null),
      orientation: patch.orientation ?? current?.orientation ?? 1,
    };
  }
}

export const session = new SessionStore();

// Keep the selection valid: when `filtered` changes (filters, mirror mode,
// rescans) prune ids that are no longer visible.
$effect.root(() => {
  $effect(() => {
    const updateDay = () => { session.todayDay = new Date().toISOString().slice(0, 10); };
    const timer = setInterval(updateDay, 60000);
    window.addEventListener("focus", updateDay);
    document.addEventListener("visibilitychange", updateDay);
    return () => {
      clearInterval(timer);
      window.removeEventListener("focus", updateDay);
      document.removeEventListener("visibilitychange", updateDay);
    };
  });

  $effect(() => {
    const items = view.mode === "survey" ? session.surveyItems : session.filtered;
    const present = new Set(items.map((i) => i.id));
    const kept = [...session.selectedIds].filter((id) => present.has(id));
    if (kept.length !== session.selectedIds.size) {
      session.selectedIds = new Set(kept);
    }
  });

  $effect(() => {
    if (view.mode !== "survey") return;
    void session.surveyItems;
    void session.selectedIds.size;
    untrack(() => session.clampFocus());
  });

  // Any view-mode transition (grid <-> loupe/compare, either direction) drops
  // the multi-selection — a marquee selection is a grid-only concept, and
  // stale ids left selected while in the loupe served no purpose. Focus is
  // deliberately left untouched, so returning to the grid still scrolls back
  // to the row of whatever was shown in the loupe/compare.
  $effect(() => {
    void view.mode;
    untrack(() => session.clearSelection());
  });

  // Apply a pending focus restored from a saved session, once the matching item
  // has loaded into `filtered`. If the user moves focus first, abandon it.
  $effect(() => {
    const key = session.pendingFocusKey;
    if (key === null) return;
    if (session.focusedIndex !== -1) {
      session.pendingFocusKey = null;
      return;
    }
    const idx = session.filtered.findIndex((i) => i.relPath === key);
    if (idx >= 0) {
      session.focusedIndex = idx;
      session.selectionAnchor = idx;
      session.pendingFocusKey = null;
    }
  });

  // Persist the per-project session (sort/media/filters/focus) on any change,
  // debounced. Skipped while no project is open, the setting is off, or a
  // restore is in flight (so open-time defaults never clobber the saved blob).
  $effect(() => {
    // Read the snapshot first so every persisted value is a tracked dependency.
    const json = JSON.stringify(session.sessionSnapshot());
    pendingSaveJson = json;
    if (!catalog.project || !settings.rememberSession || session.restoring) return;
    if (saveTimer !== null) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = null;
      pendingSaveJson = null;
      void api.setSessionState(json).catch(() => {
        // persistence is best-effort
      });
    }, 400);
  });
});

// Debounce state for the persist effect above, kept at module scope so
// `flushSessionSave` (called right before a project closes or switches) can
// reach in and write out whatever change is still waiting on its timer —
// otherwise a view/filter change made just before closing the project (e.g.
// adding a grouping level, then immediately switching projects) is silently
// lost: the 400ms debounce never gets a chance to fire once the project's DB
// connection is gone.
let saveTimer: ReturnType<typeof setTimeout> | null = null;
let pendingSaveJson: string | null = null;

/** Write out a debounced session-state save immediately instead of waiting for
 *  its timer, if one is pending. Call before anything that ends the current
 *  project's session (closing it, opening a different one, quitting the app). */
export async function flushSessionSave() {
  if (saveTimer === null || pendingSaveJson === null) return;
  clearTimeout(saveTimer);
  saveTimer = null;
  const json = pendingSaveJson;
  pendingSaveJson = null;
  try {
    await api.setSessionState(json);
  } catch {
    // persistence is best-effort
  }
}

// Multi-window / background changes reconcile through the same merge.
let workRefreshTimer: ReturnType<typeof setTimeout> | null = null;
listen<{ projectRoot: string; states: CullState[] }>("state:changed", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  const generation = catalog.generation;
  session.applyStates(e.payload.states);
  // A rating/label edit dirties XMP without queueing a pending action, so the
  // queue's own `pending:changed` never fires — refresh the commit-work
  // indicator here too, debounced so a fast-culling key burst is one query.
  if (workRefreshTimer !== null) clearTimeout(workRefreshTimer);
  workRefreshTimer = setTimeout(() => {
    workRefreshTimer = null;
    if (generation === catalog.generation) void session.refreshPending();
  }, 250);
});
listen<{ projectRoot: string }>("groups:changed", (e) => {
  if (catalog.acceptEvent(e.payload)) void catalog.refresh();
});
listen<{ projectRoot: string }>("pending:changed", (e) => {
  if (catalog.acceptEvent(e.payload)) void session.refreshPending();
});
listen<{ projectRoot: string }>("commit:done", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  session.invalidateRejectionUndo();
  void catalog.refresh();
});
