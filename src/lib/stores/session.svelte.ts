import { listen } from "@tauri-apps/api/event";
import {
  api,
  type CullState,
  type ItemLite,
  type PairScope,
  type SyncFrom,
  type Targets,
} from "../api";
import { catalog } from "./catalog.svelte";
import { tags } from "./tags.svelte";

export type FlagFilter = "all" | "pick" | "reject" | "unflagged";

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
  /** Whether the Filters dropdown panel is open. */
  filtersPanelOpen = $state(false);

  /** True when any filter narrows the grid (used to badge the Filters button). */
  hasActiveFilters = $derived(
    this.flagFilter !== "all" ||
      this.minRating > 0 ||
      this.labelFilter !== null ||
      this.tagFilter !== null,
  );

  /** Reset every filter to its neutral value. */
  clearFilters() {
    this.flagFilter = "all";
    this.minRating = 0;
    this.labelFilter = null;
    this.tagFilter = null;
    this.clampFocus();
  }
  /** Relative directory path to scope the grid to (descendants included); null = all folders combined. */
  folderFilter = $state<string | null>(null);
  folderTreeVisible = $state(true);
  /** Width of the folder-tree panel in px (persisted, drag-resizable). */
  folderTreeWidth = $state<number>(loadNumPref(FOLDER_TREE_WIDTH_KEY, 210));

  setFolderTreeWidth(px: number) {
    this.folderTreeWidth = px;
    saveNumPref(FOLDER_TREE_WIDTH_KEY, px);
  }

  // --- mirror mode (M4 flips the display; fan-out is live already) ---
  mirrorMode = $state(true);

  // --- focus / selection (indexes into `filtered`) ---
  focusedIndex = $state(0);
  /** Column count reported by the grid so ↑/↓ move one visual row. */
  gridCols = $state(1);

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

  clampFocus() {
    const max = Math.max(0, this.filtered.length - 1);
    if (this.focusedIndex > max) this.focusedIndex = max;
    if (this.focusedIndex < 0) this.focusedIndex = 0;
  }

  /** Re-keyed whenever a navigation is blocked at the first/last item, so the
   *  active view can play a quick "no more" bounce. `dir` is -1 (start) / +1
   *  (end); `axis` is the movement axis; `n` increments to re-trigger. */
  edgeBump = $state<{ dir: 1 | -1; axis: "x" | "y"; n: number }>({
    dir: 1,
    axis: "x",
    n: 0,
  });

  moveFocus(delta: number) {
    const max = Math.max(0, this.filtered.length - 1);
    const next = Math.min(max, Math.max(0, this.focusedIndex + delta));
    if (next === this.focusedIndex && delta !== 0) {
      // Already at the edge — signal a bounce instead of a silent no-op.
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
    const advance = (caps || this.autoAdvancePref) !== shift; // XOR: shift inverts
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

  async refreshPending() {
    const pending = await api.listPending();
    this.pendingCount = pending.length;
    this.pendingDeleteIds = new Set(
      pending.filter((p) => p.action === "delete").map((p) => p.fileId)
    );
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
});

// Multi-window / background changes reconcile through the same merge.
listen<CullState[]>("state:changed", (e) => session.applyStates(e.payload));
listen("groups:changed", () => catalog.refresh());
listen("pending:changed", () => session.refreshPending());
listen("commit:done", () => catalog.refresh());
