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
  filters?: {
    flagFilter?: FlagFilter;
    minRating?: number;
    labelFilter?: string | null;
    tagFilter?: number | null;
    typeFilter?: TypeFilter;
    extFilter?: string | null;
    orientationFilter?: OrientationFilter;
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

class SessionStore {
  // --- filters ---
  flagFilter = $state<FlagFilter>("all");
  minRating = $state(0);
  labelFilter = $state<string | null>(null);
  tagFilter = $state<number | null>(null);
  /** Photo file-type composition filter; inert on the videos tab. */
  typeFilter = $state<TypeFilter>("all");
  /** Single file-extension filter (lowercased, e.g. "cr3"); null = any. In
   *  mirror mode a pair matches when *any* of its members has the extension. */
  extFilter = $state<string | null>(null);
  /** Displayed-aspect orientation filter. */
  orientationFilter = $state<OrientationFilter>("all");
  /** Whether the Filters dropdown panel is open. */
  filtersPanelOpen = $state(false);

  /** True when any filter narrows the grid (used to badge the Filters button).
   *  The type filter only counts while on the photos tab (it is inert on
   *  videos), so switching tabs never leaves a phantom "active" badge. */
  hasActiveFilters = $derived(
    this.flagFilter !== "all" ||
      this.minRating > 0 ||
      this.labelFilter !== null ||
      this.tagFilter !== null ||
      (this.typeFilter !== "all" && catalog.media === "photos") ||
      this.extFilter !== null ||
      this.orientationFilter !== "all",
  );

  /** Reset every filter to its neutral value. */
  clearFilters() {
    this.flagFilter = "all";
    this.minRating = 0;
    this.labelFilter = null;
    this.tagFilter = null;
    this.typeFilter = "all";
    this.extFilter = null;
    this.orientationFilter = "all";
    this.clampFocus();
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
    if (this.folderFilter !== null) {
      out = out.filter((i) => isInFolder(i.relPath, this.folderFilter!));
    }
    return out;
  });

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
      filters: {
        flagFilter: this.flagFilter,
        minRating: this.minRating,
        labelFilter: this.labelFilter,
        tagFilter: this.tagFilter,
        typeFilter: this.typeFilter,
        extFilter: this.extFilter,
        orientationFilter: this.orientationFilter,
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
      const f = s.filters;
      if (f) {
        if (f.flagFilter) this.flagFilter = f.flagFilter;
        if (typeof f.minRating === "number") this.minRating = f.minRating;
        this.labelFilter = f.labelFilter ?? null;
        this.tagFilter = f.tagFilter ?? null;
        if (f.typeFilter) this.typeFilter = f.typeFilter;
        this.extFilter = f.extFilter ?? null;
        if (f.orientationFilter) this.orientationFilter = f.orientationFilter;
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
    this.focusedIndex = next;
    this.selectionAnchor = this.focusedIndex;
  }

  focusEdge(end: boolean) {
    this.focusedIndex = end ? Math.max(0, this.filtered.length - 1) : 0;
    this.selectionAnchor = this.focusedIndex;
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
    this.focusedIndex = index;
    this.selectionAnchor = index;
  }

  /** Shift+click: contiguous range from the anchor. `additive` = Ctrl held. */
  rangeSelect(index: number, additive = false) {
    if (this.filtered.length === 0) return;
    const anchor = this.selectionAnchor ?? this.focusedIndex;
    const lo = Math.min(anchor, index);
    const hi = Math.max(anchor, index);
    const next = additive ? new Set(this.selectedIds) : new Set<number>();
    for (let i = lo; i <= hi; i++) {
      const item = this.filtered[i];
      if (item) next.add(item.id);
    }
    this.selectedIds = next;
    this.focusedIndex = index;
    this.selectionAnchor = anchor;
  }

  clearSelection() {
    if (this.selectedIds.size > 0) this.selectedIds = new Set();
    this.selectionAnchor = null;
  }

  selectAll() {
    this.selectedIds = new Set(this.filtered.map((i) => i.id));
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
  commitDialogOpen = $state(false);
  moveDialogOpen = $state(false);
  /** Result of the most recent commit, surfaced as a popup AFTER the commit
   *  dialog closes (null = nothing to announce). */
  commitDone = $state<{ title: string; message: string } | null>(null);

  async refreshPending() {
    const pending = await api.listPending();
    this.pendingCount = pending.length;
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
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    // Read the snapshot first so every persisted value is a tracked dependency.
    const json = JSON.stringify(session.sessionSnapshot());
    if (!catalog.project || !settings.rememberSession || session.restoring) return;
    if (saveTimer !== null) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = null;
      void api.setSessionState(json).catch(() => {
        // persistence is best-effort
      });
    }, 400);
  });
});

// Multi-window / background changes reconcile through the same merge.
listen<CullState[]>("state:changed", (e) => session.applyStates(e.payload));
listen("groups:changed", () => catalog.refresh());
listen("pending:changed", () => session.refreshPending());
listen("commit:done", () => catalog.refresh());
