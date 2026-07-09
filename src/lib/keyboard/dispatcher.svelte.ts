import { session } from "../stores/session.svelte";
import { tags } from "../stores/tags.svelte";
import { view } from "../stores/view.svelte";
import {
  effectiveBindings,
  loadOverrides,
  normalizeKey,
  saveOverrides,
  type CommandId,
} from "./keymap";

class KeymapStore {
  overrides = $state<Partial<Record<CommandId, string[]>>>(loadOverrides());
  bindings = $derived(effectiveBindings(this.overrides));
  /** When set, the next keydown rebinds this command instead of executing. */
  rebinding = $state<CommandId | null>(null);

  rebind(command: CommandId, key: string) {
    this.overrides = { ...this.overrides, [command]: [key] };
    saveOverrides(this.overrides);
  }

  reset() {
    this.overrides = {};
    saveOverrides(this.overrides);
  }
}

export const keymap = new KeymapStore();

function execute(id: CommandId, e?: KeyboardEvent) {
  switch (id) {
    case "nav.next":
      return session.moveFocus(1);
    case "nav.prev":
      return session.moveFocus(-1);
    case "nav.down":
      return session.moveFocus(session.gridCols ?? 1);
    case "nav.up":
      return session.moveFocus(-(session.gridCols ?? 1));
    case "nav.home":
      return session.focusEdge(false);
    case "nav.end":
      return session.focusEdge(true);
    case "select.all":
      return session.selectAll();
    case "rate.0":
    case "rate.1":
    case "rate.2":
    case "rate.3":
    case "rate.4":
    case "rate.5":
      return session.rate(Number(id.slice(5)), e);
    case "flag.pick":
      return session.flag(1, e);
    case "flag.reject":
      return session.flag(-1, e);
    case "flag.unflag":
      return session.flag(0, e);
    case "flag.toggle":
      return session.toggleFlag(e);
    case "label.red":
      return session.label("Red", e);
    case "label.yellow":
      return session.label("Yellow", e);
    case "label.green":
      return session.label("Green", e);
    case "label.blue":
      return session.label("Blue", e);
    case "label.purple":
      return session.label("Purple", e);
    case "ui.toggleFilterBar":
      session.filterBarVisible = !session.filterBarVisible;
      return;
    case "ui.toggleFolderTree":
      session.folderTreeVisible = !session.folderTreeVisible;
      return;
    case "ui.toggleMirror":
      session.setMirrorMode(!session.mirrorMode);
      return;
    case "view.grid":
      view.mode = "grid";
      return;
    case "view.viewer":
      view.mode = "viewer";
      return;
    case "view.compare":
      view.mode = "compare";
      return;
    case "view.back":
      // Esc returns to the grid from loupe/compare; in grid it clears selection.
      if (view.mode !== "grid") view.mode = "grid";
      else session.clearSelection();
      return;
    case "zoom.toggle":
      if (view.mode !== "grid") view.toggleZoom();
      return;
    case "pair.toggleShown":
      return session.togglePairHalf();
    case "pair.toggleCoupling":
      return void session.togglePairCoupling();
    case "tag.chord":
      armTagChord();
      return;
    case "delete.pair":
      return void session.queueDelete("both", e);
    case "delete.rawOnly":
      return void session.queueDelete("rawonly", e);
    case "delete.jpegOnly":
      return void session.queueDelete("jpegonly", e);
    case "action.moveCopy":
      session.moveDialogOpen = true;
      return;
    case "commit.open":
      session.commitDialogOpen = true;
      return;
    case "info.toggle":
      // Only meaningful in the loupe; harmless elsewhere.
      view.infoOpen = !view.infoOpen;
      return;
    case "ui.toggleFilmstrip":
      session.toggleShowFilmstrip();
      return;
  }
}

// --- T + digit chord for task tags ---
let chordTimer: ReturnType<typeof setTimeout> | null = null;

export const chord = $state({ armed: false });

function armTagChord() {
  chord.armed = true;
  if (chordTimer) clearTimeout(chordTimer);
  chordTimer = setTimeout(() => (chord.armed = false), 2000);
}

function handleChord(e: KeyboardEvent): boolean {
  if (!chord.armed) return false;
  chord.armed = false;
  if (chordTimer) clearTimeout(chordTimer);
  const n = Number(e.key);
  if (Number.isInteger(n) && n >= 1 && n <= 9) {
    const tag = tags.all[n - 1];
    if (tag) void session.toggleTag(tag.id, e);
    return true; // digit consumed by the chord, never reaches rate.N
  }
  return false; // any other key cancels the chord and runs normally
}

/** Run a command outside the keyboard path (e.g. from a touch button).
 * No KeyboardEvent, so classification commands never invert auto-advance. */
export function runCommand(id: CommandId) {
  execute(id);
}

export function handleKeydown(e: KeyboardEvent) {
  const target = e.target as HTMLElement | null;
  if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;

  const normalized = normalizeKey(e);

  if (keymap.rebinding) {
    if (normalized !== "escape") keymap.rebind(keymap.rebinding, normalized);
    keymap.rebinding = null;
    e.preventDefault();
    return;
  }

  if (handleChord(e)) {
    e.preventDefault();
    return;
  }

  // Explicit binding (with shift) wins; otherwise retry without shift —
  // there shift only means "invert auto-advance" for classification keys.
  const command =
    keymap.bindings.get(normalized) ?? keymap.bindings.get(normalizeKey(e, false));
  if (command) {
    e.preventDefault();
    execute(command, e);
    return;
  }

  // Per-tag custom shortcuts (assigned in the tag editor).
  const tag = tags.all.find((t) => t.shortcut === normalized);
  if (tag) {
    e.preventDefault();
    void session.toggleTag(tag.id, e);
  }
}
