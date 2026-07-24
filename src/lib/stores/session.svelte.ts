import { listen } from "@tauri-apps/api/event";
import { untrack } from "svelte";
import {
  api,
  type CullState,
  type ItemLite,
  type MediaTab,
  type PairScope,
  type SortKey,
  type SyncFrom,
  type Targets,
} from "../api";
import { adaptiveGap, computeBursts } from "../bursts";
import { groupCompare, type GroupContext } from "../gridGroups";
import { apertureBucket, focalBucket, isoBucket, shutterBucket } from "../metadataFacets";
import { catalog } from "./catalog.svelte";
import { settings } from "./settings.svelte";
import { tags } from "./tags.svelte";
import { view } from "./view.svelte";

export type FlagFilter = "all" | "pick" | "reject" | "unflagged";
/** Photo file-type composition filter (photos tab only). */
export type TypeFilter = "all" | "raw" | "jpeg" | "rawjpeg";
/** Displayed-aspect orientation filter (applies to photos and videos). */
export type OrientationFilter = "all" | "portrait" | "landscape" | "square";

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
    cameraFilter?: string | null;
    lensFilter?: string | null;
    isoFilter?: string | null;
    apertureFilter?: string | null;
    focalFilter?: string | null;
    shutterFilter?: string | null;
    folderFilter?: string | null;
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

/** Whether a file's directory is `folder` itself or one of its descendants. */
function isInFolder(relPath: string, folder: string): boolean {
  const dir = dirOf(relPath);
  return dir === folder || dir.startsWith(`${folder}/`);
}

export const LABELS = ["Red", "Yellow", "Green", "Blue", "Purple"] as const;
export type Label = (typeof LABELS)[number];

/** Grid thumbnail size. `medium` is the historical default. */
export type GridDensity = "small" | "medium" | "large";

class SessionStore {
  // --- filters ---
  flagFilter = $state<FlagFilter>("all");
  minRating = $state(0);
  labelFilter = $state<string | null>(null);
  tagFilter = $state<number | null>(null);
  /** Case-insensitive substring match on the file name; "" = no filter. Shared
   *  by the search overlay and the Sort & Filter field, so whichever one the
   *  user opens shows what the other typed. */
  nameFilter = $state("");
  /** Photo file-type composition filter; inert on the videos tab. */
  typeFilter = $state<TypeFilter>("all");
  /** Single file-extension filter (lowercased, e.g. "cr3"); null = any. In
   *  mirror mode a pair matches when *any* of its members has the extension. */
  extFilter = $state<string | null>(null);
  /** Displayed-aspect orientation filter. */
  orientationFilter = $state<OrientationFilter>("all");
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
  /** Whether the Filters dropdown panel is open. */
  filtersPanelOpen = $state(false);
  /** Whether the name-search overlay is open (Ctrl+F). */
  searchOpen = $state(false);

  /** True when any filter narrows the grid (used to badge the Filters button).
   *  The type filter only counts while on the photos tab (it is inert on
   *  videos), so switching tabs never leaves a phantom "active" badge. */
  hasActiveFilters = $derived(
    this.flagFilter !== "all" ||
      this.minRating > 0 ||
      this.labelFilter !== null ||
      this.tagFilter !== null ||
      this.nameFilter.trim() !== "" ||
      (this.typeFilter !== "all" && catalog.media === "photos") ||
      this.extFilter !== null ||
      this.orientationFilter !== "all" ||
      // The five photographic-settings filters are photo-only, so they never
      // badge the button while the videos tab is active (matching typeFilter).
      ((this.cameraFilter !== null ||
        this.lensFilter !== null ||
        this.isoFilter !== null ||
        this.apertureFilter !== null ||
        this.focalFilter !== null ||
        this.shutterFilter !== null) &&
        catalog.media === "photos"),
  );

  /** Reset every filter to its neutral value. */
  clearFilters() {
    this.flagFilter = "all";
    this.minRating = 0;
    this.labelFilter = null;
    this.tagFilter = null;
    this.nameFilter = "";
    this.typeFilter = "all";
    this.extFilter = null;
    this.orientationFilter = "all";
    this.cameraFilter = null;
    this.lensFilter = null;
    this.isoFilter = null;
    this.apertureFilter = null;
    this.focalFilter = null;
    this.shutterFilter = null;
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
    this.clearFilters();
    this.groupBy = [];
    this.gridDensity = "medium";
    this.stickyGroupHeader = false;
  }

  /** Relative directory path to scope the grid to (descendants included); null = all folders combined. */
  folderFilter = $state<string | null>(null);
  folderTreeVisible = $state(initialFolderTreeVisible());
  /** Width of the folder-tree panel in px (persisted, drag-resizable). */
  folderTreeWidth = $state<number>(loadNumPref(FOLDER_TREE_WIDTH_KEY, 210));

  setFolderTreeWidth(px: number) {
    this.folderTreeWidth = px;
    saveNumPref(FOLDER_TREE_WIDTH_KEY, px);
  }

  // --- mirror mode (M4 flips the display; fan-out is live already) ---
  mirrorMode = $state(true);

  // --- focus / selection (indexes into `filtered`) ---
  /** Index into `filtered`; -1 is the sentinel "no item focused" state (no
   *  grid cell matches, so nothing shows the focus outline). */
  focusedIndex = $state(-1);
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
  /** Index into `filtered` where the last explicit selection started (Shift ranges). */
  selectionAnchor = $state<number | null>(null);
  /** Far end of the last Shift range, so a keyboard extension continues from
   *  where the range ended rather than from the anchor. */
  private rangeHead = $state<number | null>(null);

  // --- grid view (density + grouping) ---
  /** Grid thumbnail size; persisted per project via the session blob. */
  gridDensity = $state<GridDensity>("medium");
  /** Ordered grouping dimensions (keys from `gridGroups`); [] = no grouping
   *  (the default flat grid). Level 0 is the outermost section. */
  groupBy = $state<string[]>([]);
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
  groupContext = $derived<GroupContext>({ bursts: this.bursts });

  filtered = $derived.by(() => {
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
    if (this.flagFilter !== "all") {
      const want = this.flagFilter === "pick" ? 1 : this.flagFilter === "reject" ? -1 : 0;
      out = out.filter((i) => i.flag === want);
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
    const query = this.nameFilter.trim().toLowerCase();
    if (query !== "") {
      out = out.filter((i) => i.name.toLowerCase().includes(query));
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
    // Photographic-settings filters. Photo-only (a video carries no camera/lens/
    // ISO/focal/aperture EXIF), so they are inert on the videos tab. camera/lens
    // compare the exact EXIF string; iso/aperture/focal compare the item's bucket
    // key against the picked one, using the very functions the panel built the
    // chips from. In mirror mode `out` holds one entry per pair, but both halves
    // of a RAW+JPEG pair share the same shot, so testing the shown member is exact.
    if (catalog.media === "photos") {
      if (this.cameraFilter !== null) {
        out = out.filter((i) => i.camera === this.cameraFilter);
      }
      if (this.lensFilter !== null) {
        out = out.filter((i) => i.lens === this.lensFilter);
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
    if (this.folderFilter !== null) {
      out = out.filter((i) => isInFolder(i.relPath, this.folderFilter!));
    }
    // Grouping is a stable multi-level sort applied last, so items of the same
    // bucket become contiguous while the active catalog sort survives within the
    // deepest bucket. Keeping it in `filtered` (not just the grid) means the
    // focus/selection index space matches what the grid draws. Copy first: `out`
    // may still alias catalog.items here (separate mode with no active filter).
    if (this.groupBy.length > 0) {
      const ctx = this.groupContext;
      out = [...out].sort((a, b) => groupCompare(a, b, this.groupBy, ctx));
    }
    return out;
  });

  /** Where the focused photo sits in its burst, or null when it is not in one.
   *  Counted over `filtered`, so the position matches what stepping with , and .
   *  will actually walk through. */
  focusedBurst = $derived.by(() => {
    const item = this.focused;
    if (!item) return null;
    const key = this.bursts.byFile.get(item.id);
    if (!key) return null;
    const members: number[] = [];
    let index = -1;
    for (let i = 0; i < this.filtered.length; i++) {
      if (this.bursts.byFile.get(this.filtered[i].id) !== key) continue;
      if (i === this.focusedIndex) index = members.length;
      members.push(i);
    }
    if (index === -1 || members.length < 2) return null;
    return { position: index + 1, total: members.length, members };
  });

  /** Step within the focused photo's burst (`,` and `.`), stopping at its own
   *  ends rather than walking on into the next one. */
  stepBurst(delta: number) {
    const burst = this.focusedBurst;
    if (!burst) return;
    const next = burst.position - 1 + delta;
    if (next < 0 || next >= burst.members.length) return;
    this.collapseSelection();
    this.focusedIndex = burst.members[next];
    this.selectionAnchor = this.focusedIndex;
  }

  // --- survey (N-up elimination) ---

  /** Candidate file ids while the survey is open. It SHRINKS as photos are
   *  eliminated — that is the whole point of the view, and why this is its own
   *  list rather than a filter over `filtered`. */
  surveyIds = $state<number[]>([]);

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

  /** How many photos were asked for, when that exceeded MAX_SURVEY; 0 when the
   *  whole set fits. Surfaced in the view rather than truncating in silence. */
  surveyRequested = $state(0);

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
  canSurvey = $derived(this.selectedIds.size >= 2 || this.focusedBurst !== null);

  openSurvey() {
    const all = this.surveyCandidates();
    if (all.length < 2) return;
    this.surveyRequested = all.length > SessionStore.MAX_SURVEY ? all.length : 0;
    const ids = all.slice(0, SessionStore.MAX_SURVEY);
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
    this.surveyIds = [];
    this.surveyRequested = 0;
    view.mode = "grid";
  }

  /** Move focus to the next/previous survey candidate, stopping at the ends. */
  stepSurvey(delta: number) {
    const items = this.surveyItems;
    if (items.length === 0) return;
    const current = items.findIndex((i) => i.id === this.focused?.id);
    const next = Math.min(items.length - 1, Math.max(0, current + delta));
    if (next === current) return;
    const idx = this.filtered.findIndex((i) => i.id === items[next].id);
    if (idx >= 0) this.focusedIndex = idx;
  }

  /** Focus a survey tile directly (click/tap). */
  focusSurveyItem(id: number) {
    const idx = this.filtered.findIndex((i) => i.id === id);
    if (idx >= 0) this.focusedIndex = idx;
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
      this.closeSurvey();
      return;
    }
    // Prefer whatever slid into the vacated slot, else the new last one.
    const target = left[Math.min(at < 0 ? 0 : at, left.length - 1)];
    this.focusSurveyItem(target.id);
  }

  /** Flip which half of the focused pair is displayed (J). */
  togglePairHalf() {
    const item = this.focused;
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
    for (const i of catalog.items) {
      if (i.flag === 1) pick++;
      else if (i.flag === -1) reject++;
      else unflagged++;
    }
    return { pick, reject, unflagged, total: catalog.items.length };
  });

  // --- per-project session persistence (migration v4) ---

  /** Serialise the current view + filters + focus for persistence. */
  sessionSnapshot(): SavedSession {
    return {
      sort: catalog.sort,
      sortDesc: catalog.sortDesc,
      media: catalog.media,
      gridDensity: this.gridDensity,
      groupBy: this.groupBy,
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
        cameraFilter: this.cameraFilter,
        lensFilter: this.lensFilter,
        isoFilter: this.isoFilter,
        apertureFilter: this.apertureFilter,
        focalFilter: this.focalFilter,
        shutterFilter: this.shutterFilter,
        folderFilter: this.folderFilter,
      },
      focusKey: this.focused?.relPath ?? null,
    };
  }

  /** Load the saved per-project session (if the setting is on) and apply the
   *  sort/media/filters. Focus is deferred to `pendingFocusKey`, applied once a
   *  matching item loads. Best-effort: any error or missing/invalid blob leaves
   *  the defaults chosen at open time untouched. */
  async restoreSessionState() {
    try {
      if (!settings.rememberSession) return;
      let raw: string | null;
      try {
        raw = await api.getSessionState();
      } catch {
        return;
      }
      if (!raw) return;
      let s: SavedSession;
      try {
        s = JSON.parse(raw) as SavedSession;
      } catch {
        return;
      }
      if (s.gridDensity) this.gridDensity = s.gridDensity;
      if (Array.isArray(s.groupBy)) this.groupBy = s.groupBy.filter((k) => typeof k === "string");
      if (typeof s.stickyGroupHeader === "boolean") this.stickyGroupHeader = s.stickyGroupHeader;
      const f = s.filters;
      if (f) {
        if (f.flagFilter) this.flagFilter = f.flagFilter;
        if (typeof f.minRating === "number") this.minRating = f.minRating;
        this.labelFilter = f.labelFilter ?? null;
        this.tagFilter = f.tagFilter ?? null;
        this.nameFilter = f.nameFilter ?? "";
        if (f.typeFilter) this.typeFilter = f.typeFilter;
        this.extFilter = f.extFilter ?? null;
        if (f.orientationFilter) this.orientationFilter = f.orientationFilter;
        this.cameraFilter = f.cameraFilter ?? null;
        this.lensFilter = f.lensFilter ?? null;
        this.isoFilter = f.isoFilter ?? null;
        this.apertureFilter = f.apertureFilter ?? null;
        this.focalFilter = f.focalFilter ?? null;
        this.shutterFilter = f.shutterFilter ?? null;
        this.folderFilter = f.folderFilter ?? null;
      }
      this.pendingFocusKey = s.focusKey ?? null;
      // Sort/media re-query the catalog to reorder/reselect the visible items.
      if (s.sort || s.media || typeof s.sortDesc === "boolean") {
        await catalog.applyRestoredView(s.sort, s.sortDesc, s.media);
      }
      this.clampFocus();
    } finally {
      this.restoring = false;
    }
  }

  clampFocus() {
    const max = Math.max(0, this.filtered.length - 1);
    if (this.focusedIndex > max) this.focusedIndex = max;
    // Preserve a deliberate -1 ("nothing focused"); only pull other
    // out-of-range negatives up into the valid range.
    else if (this.focusedIndex < -1) this.focusedIndex = 0;
  }

  /** Enter the "no item focused" state: no grid cell shows the focus outline,
   *  and any active selection/anchor is dropped. Used on project open and when
   *  switching the Photos/Videos media tab. */
  clearFocus() {
    this.focusedIndex = -1;
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

  moveFocus(delta: number) {
    const max = Math.max(0, this.filtered.length - 1);
    const next = Math.min(max, Math.max(0, this.focusedIndex + delta));
    if (next === this.focusedIndex && delta !== 0) {
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
    this.focusedIndex = end ? Math.max(0, this.filtered.length - 1) : 0;
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
    if (this.focusedIndex === -1 && this.filtered.length > 0) {
      this.focusedIndex = 0;
      this.selectionAnchor = 0;
    }
  }

  // --- multi-selection (Windows-style) ---

  /** Plain click: focus only, drop any selection. */
  selectOnly(index: number) {
    this.focusedIndex = index;
    this.selectionAnchor = index;
    if (this.selectedIds.size > 0) this.selectedIds = new Set();
  }

  /** Ctrl+click: toggle one item; an empty selection is seeded from focus. */
  toggleSelect(index: number) {
    const item = this.filtered[index];
    if (!item) return;
    const wasEmpty = this.selectedIds.size === 0;
    const next = new Set(this.selectedIds);
    if (wasEmpty && this.focused) next.add(this.focused.id);
    // Ctrl+clicking the focused item of an empty selection selects it —
    // seed + toggle would cancel out, so skip the toggle in that one case.
    if (!(wasEmpty && this.focused?.id === item.id)) {
      if (next.has(item.id)) next.delete(item.id);
      else next.add(item.id);
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
    const lo = Math.min(anchor, head);
    const hi = Math.max(anchor, head);
    const next = additive ? new Set(this.selectedIds) : new Set<number>();
    for (let i = lo; i <= hi; i++) {
      const item = this.filtered[i];
      if (item) next.add(item.id);
    }
    this.selectedIds = next;
  }

  /** Shift+click: contiguous range from the anchor. `additive` = Ctrl held. */
  rangeSelect(index: number, additive = false) {
    if (this.filtered.length === 0) return;
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
    const max = this.filtered.length - 1;
    const next = Math.min(max, Math.max(0, head + delta));
    if (next === head) return;
    this.extendTo(next);
  }

  /** Shift+Home / Shift+End: extend the selection to the first/last item. */
  extendToEdge(end: boolean) {
    if (this.filtered.length === 0) return;
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
    this.selectedIds = new Set(this.filtered.map((i) => i.id));
    // No focus while a selection exists (see toggleSelect).
    this.focusedIndex = -1;
  }

  /** Select everything the current selection leaves out. */
  invertSelection() {
    const next = new Set<number>();
    for (const item of this.filtered) {
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

  private targets(): Targets | null {
    if (this.selectedIds.size > 0) {
      return { ids: [...this.selectedIds], asGroups: this.mirrorMode };
    }
    const item = this.focused;
    if (!item) return null;
    return { ids: [item.id], asGroups: this.mirrorMode };
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

  async rate(rating: number, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    this.applyStates(t.ids.map((id) => this.localGuess(id, { rating })));
    this.maybeAdvance(event);
    this.applyStates(await api.setRating(t, rating));
  }

  async flag(flag: number, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    this.applyStates(t.ids.map((id) => this.localGuess(id, { flag })));
    this.maybeAdvance(event);
    this.applyStates(await api.setFlag(t, flag));
    // Invariant: reject flag = queued delete. Every flag write funnels through
    // here, so this is the single place that keeps the two in sync: rejecting
    // enqueues a delete, un-rejecting/picking removes it. Same Targets as the
    // flag write, so the backend pair fan-out matches.
    if (flag === -1) await api.enqueueAction(t, "delete", null, "both");
    else await api.removePendingForFiles(t, "delete");
    // In the survey, rejecting is how you eliminate: the photo leaves the
    // running and the survivors grow. Hooked here rather than in the view so it
    // holds for every route to a reject — key, action bar or touch.
    if (flag === -1 && view.mode === "survey") this.eliminateFromSurvey(t.ids);
    // The pending:changed listener refreshes too; this keeps ordering explicit.
    await this.refreshPending();
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
    this.applyStates(t.ids.map((id) => this.localGuess(id, { label: next })));
    this.maybeAdvance(event);
    this.applyStates(await api.setLabel(t, next));
  }

  /** Apply a label (null clears) exactly as given — no toggle. The bars use
   *  this: they already resolve set-vs-clear from the selection-uniform state
   *  they display, unlike the keyboard path (`label`), which toggles off the
   *  focused item Lightroom-style. */
  async setLabel(label: string | null) {
    const t = this.targets();
    if (!t) return;
    this.applyStates(t.ids.map((id) => this.localGuess(id, { label })));
    // Fast culling advances on any classification from the bars too, matching the
    // keyboard `label()` path. No-ops outside fast culling / outside loupe-compare.
    this.maybeAdvance();
    this.applyStates(await api.setLabel(t, label));
  }

  /** Wipe ALL classification off the current targets in one action: rating,
   *  flag, color label and every task tag. Clearing the flag also unqueues the
   *  delete a reject implies (the same reject-flag = queued-delete invariant the
   *  `flag()` choke point maintains). Never auto-advances — a reset is not a
   *  cull step. */
  async clearClassification() {
    const t = this.targets();
    if (!t) return;
    this.applyStates(t.ids.map((id) => this.localGuess(id, { rating: 0, flag: 0, label: null })));
    this.applyStates(await api.setRating(t, 0));
    this.applyStates(await api.setFlag(t, 0));
    this.applyStates(await api.setLabel(t, null));
    await api.removePendingForFiles(t, "delete");
    tags.applyChanges(await api.clearTaskTags(t));
    await this.refreshPending();
  }

  /** Turn the current targets a quarter turn; positive `steps` is clockwise.
   *  The new orientation comes back from the backend instead of being guessed
   *  locally, so the EXIF rotation table lives in exactly one place. */
  async rotate(steps: number) {
    const t = this.targets();
    if (!t) return;
    this.applyStates(await api.rotate(t, steps));
  }

  /** Toggle a task tag on the focused photo (fan-out included). */
  async toggleTag(tagId: number, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    this.maybeAdvance(event);
    tags.applyChanges(await api.toggleTaskTag(t, tagId));
  }

  // --- pending actions ---
  /** File ids with a queued delete (for grid badges). */
  pendingDeleteIds = $state<Set<number>>(new Set());
  pendingCount = $state(0);
  /** Photos with a pending XMP sidecar write. Tracked separately because XMP
   *  dirtiness is a per-file flag, not a pending-actions row — a plain rating or
   *  label edit leaves nothing in the queue yet still needs a commit. */
  xmpDirtyCount = $state(0);
  commitDialogOpen = $state(false);
  moveDialogOpen = $state(false);
  /** Result of the most recent commit, surfaced as a popup AFTER the commit
   *  dialog closes (null = nothing to announce). */
  commitDone = $state<{ title: string; message: string } | null>(null);

  /** Whether the commit button should light up: anything queued OR any pending
   *  XMP write. This is the full set of work a commit would perform. */
  hasCommitWork = $derived(this.pendingCount > 0 || this.xmpDirtyCount > 0);

  async refreshPending() {
    const [pending, xmpDirty] = await Promise.all([api.listPending(), api.xmpDirtyCount()]);
    this.pendingCount = pending.length;
    this.xmpDirtyCount = xmpDirty;
    this.pendingDeleteIds = new Set(
      pending.filter((p) => p.action === "delete").map((p) => p.fileId)
    );
  }

  /** Reconcile rejects made before the reject-flag/delete-queue sync existed:
   *  enqueue a delete for every file in the catalog still at flag -1. Runs once
   *  per project open; enqueueAction upserts, so it's idempotent. Queries both
   *  media tabs (the full catalog) rather than the currently loaded one. */
  async syncRejectedToQueue() {
    const ids: number[] = [];
    for (const media of ["photos", "videos"] as const) {
      const items = await api.queryItems("capture", media, false);
      for (const i of items) if (i.flag === -1) ids.push(i.id);
    }
    if (ids.length === 0) return;
    await api.enqueueAction({ ids, asGroups: true }, "delete", null, "both");
    await this.refreshPending();
  }

  /** Queue a delete for the focused photo. Scope picks pair members. */
  async queueDelete(scope: PairScope, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    await api.enqueueAction(t, "delete", null, scope);
    this.maybeAdvance(event);
    await this.refreshPending();
  }

  /** Group id awaiting a recouple sync choice (renders PairSyncDialog). */
  recoupleDialogFor = $state<number | null>(null);

  /** Ctrl+J: decouple a linked pair, or start recoupling a split one. */
  async togglePairCoupling() {
    const item = this.focused;
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

  private localGuess(id: number, patch: Partial<CullState>): CullState {
    const current = catalog.items.find((i) => i.id === id);
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
    const present = new Set(session.filtered.map((i) => i.id));
    const kept = [...session.selectedIds].filter((id) => present.has(id));
    if (kept.length !== session.selectedIds.size) {
      session.selectedIds = new Set(kept);
    }
  });

  // A different project's folders have nothing to do with the last one's.
  $effect(() => {
    void catalog.project;
    session.folderFilter = null;
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
listen<CullState[]>("state:changed", (e) => {
  session.applyStates(e.payload);
  // A rating/label edit dirties XMP without queueing a pending action, so the
  // queue's own `pending:changed` never fires — refresh the commit-work
  // indicator here too, debounced so a fast-culling key burst is one query.
  if (workRefreshTimer !== null) clearTimeout(workRefreshTimer);
  workRefreshTimer = setTimeout(() => {
    workRefreshTimer = null;
    void session.refreshPending();
  }, 250);
});
listen("groups:changed", () => catalog.refresh());
listen("pending:changed", () => session.refreshPending());
listen("commit:done", () => catalog.refresh());
