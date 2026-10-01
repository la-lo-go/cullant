import { session } from "../stores/session.svelte";
import { tags } from "../stores/tags.svelte";
import { view } from "../stores/view.svelte";
import {
  effectiveBindings,
  COMMANDS,
  DEFAULT_BINDINGS,
  isModifierKey,
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
    const overrides = { ...this.overrides, [command]: [key] };
    for (const { id } of COMMANDS) {
      if (id === command) continue;
      const keys = overrides[id] ?? DEFAULT_BINDINGS[id];
      if (keys.includes(key)) overrides[id] = keys.filter((binding) => binding !== key);
    }
    this.overrides = overrides;
    saveOverrides(this.overrides);
  }

  reset() {
    this.overrides = {};
    this.rebinding = null;
    saveOverrides(this.overrides);
  }

  resetCommand(command: CommandId) {
    const overrides = { ...this.overrides };
    delete overrides[command];
    this.overrides = overrides;
    this.rebinding = null;
    saveOverrides(this.overrides);
  }
}

export const keymap = new KeymapStore();

const NO_REPEAT = new Set<CommandId>([
  "undo.rejection",
  "flag.toggle", "label.red", "label.yellow", "label.green", "label.blue", "label.purple",
  "ui.toggleFilterBar", "ui.search", "ui.toggleFolderTree", "ui.toggleMirror", "ui.toggleFilmstrip", "ui.toggleShortcuts",
  "view.fullscreen", "info.toggle", "pair.toggleShown", "pair.toggleCoupling", "edit.rotateLeft", "edit.rotateRight",
]);

function execute(id: CommandId, e?: KeyboardEvent) {
  switch (id) {
    case "undo.rejection":
      if (view.mode === "survey") void session.undoSurveyRejection();
      else if (view.mode !== "grid") void session.undoLastRejection();
      return;
    // Shift on a navigation key extends the selection instead of moving focus.
    // It keeps its auto-advance-inverter meaning on classification keys, which
    // go through `maybeAdvance` on a separate path.
    //
    // In the survey, arrows walk the candidates rather than the whole catalog:
    // stepping out of the field on show would defeat the point of the view.
    case "nav.next":
      return e?.shiftKey ? session.extendSelection(1) : session.moveFocus(1);
    case "nav.prev":
      return e?.shiftKey ? session.extendSelection(-1) : session.moveFocus(-1);
    case "nav.down": {
      const rows = view.mode === "survey" ? 1 : session.gridCols ?? 1;
      return e?.shiftKey ? session.extendSelection(rows) : session.moveFocus(rows);
    }
    case "nav.up": {
      const rows = view.mode === "survey" ? -1 : -(session.gridCols ?? 1);
      return e?.shiftKey ? session.extendSelection(rows) : session.moveFocus(rows);
    }
    case "nav.home":
      return e?.shiftKey ? session.extendToEdge(false) : session.focusEdge(false);
    case "nav.end":
      return e?.shiftKey ? session.extendToEdge(true) : session.focusEdge(true);
    case "select.all":
      return session.selectAll();
    case "select.none":
      return session.clearSelection();
    case "select.invert":
      return session.invertSelection();
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
      session.filtersPanelOpen = !session.filtersPanelOpen;
      return;
    case "ui.search":
      session.searchOpen = !session.searchOpen;
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
      if (view.mode === "survey") return session.inspectSurvey();
      session.ensureFocus();
      view.mode = "viewer";
      return;
    case "view.compare":
      session.ensureFocus();
      view.mode = "compare";
      return;
    case "view.survey":
      // A no-op without at least two candidates; `canSurvey` is what the entry
      // points gate on so the key is never offered as a dead end.
      session.openSurvey();
      return;
    case "view.back":
      // Esc returns to the grid from loupe/compare; in grid it clears selection.
      if (view.mode === "survey") {
        if (!session.leaveSurveyDetail()) session.closeSurvey();
      }
      else if (view.mode !== "grid") view.mode = "grid";
      else session.clearSelection();
      return;
    case "view.fullscreen":
      // Full screen is a loupe/compare affordance; a no-op in the grid.
      if (view.mode !== "grid") view.toggleFullscreen();
      return;
    case "zoom.toggle":
      if (view.mode !== "grid") view.requestZoomToggle();
      return;
    case "zoom.in":
      if (view.mode !== "grid") view.requestZoomStep(1);
      return;
    case "zoom.out":
      if (view.mode !== "grid") view.requestZoomStep(-1);
      return;
    case "burst.prev":
      return session.stepBurst(-1);
    case "burst.next":
      return session.stepBurst(1);
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
    case "edit.rotateLeft":
      return void session.rotate(-1);
    case "edit.rotateRight":
      return void session.rotate(1);
    case "action.moveCopy":
      session.moveDialogTargets = null;
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
    case "ui.toggleShortcuts":
      // Ignore key auto-repeat: holding `?` would otherwise flip the cheat-sheet
      // open/closed many times a second.
      if (e?.repeat) return;
      view.shortcutsOpen = !view.shortcutsOpen;
      return;
  }
}

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
  session.capsLockActive = e.getModifierState("CapsLock");
  if (e.defaultPrevented) return;
  // Contain keys even when a pointer choice or the toolbar moves DOM focus out.
  if (session.filtersPanelOpen) {
    if (e.key === "Escape") {
      e.preventDefault();
      session.filtersPanelOpen = false;
    }
    return;
  }
  const target = e.target instanceof Element ? e.target : null;
  if (target?.closest('input, textarea, select, [contenteditable="true"]')) return;

  const normalized = normalizeKey(e);

  if (keymap.rebinding) {
    if (isModifierKey(e)) {
      e.preventDefault();
      return;
    }
    if (normalized !== "escape") keymap.rebind(keymap.rebinding, normalized);
    keymap.rebinding = null;
    e.preventDefault();
    return;
  }

  // The shortcuts cheat-sheet is a read-only modal: while it's open swallow all
  // keys so nothing behind it fires, honouring only the toggle key (`?`) and
  // the back/close key (Esc) to dismiss it.
  if (view.shortcutsOpen) {
    e.preventDefault();
    const cmd =
      keymap.bindings.get(normalized) ?? keymap.bindings.get(normalizeKey(e, false));
    // Ignore `?` auto-repeat here too: a held toggle key must not close-then-
    // (via the normal path) reopen the sheet, flickering it.
    if ((cmd === "ui.toggleShortcuts" && !e.repeat) || cmd === "view.back") {
      view.shortcutsOpen = false;
    }
    return;
  }

  if ((e.key === "Enter" || e.key === " ") && target?.closest('button, a[href], summary, [role="button"], [role="menuitem"], [role="switch"], [role="checkbox"], [role="radio"], [role="tab"]')) return;

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
    if (e.repeat && NO_REPEAT.has(command)) return;
    execute(command, e);
    return;
  }

  const tag = tags.all.find((t) => t.shortcut === normalized);
  if (tag) {
    e.preventDefault();
    if (e.repeat) return;
    void session.toggleTag(tag.id, e);
  }
}
