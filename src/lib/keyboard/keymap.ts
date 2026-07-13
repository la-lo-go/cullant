export type CommandId =
  | "nav.next"
  | "nav.prev"
  | "nav.down"
  | "nav.up"
  | "nav.home"
  | "nav.end"
  | "select.all"
  | "rate.0"
  | "rate.1"
  | "rate.2"
  | "rate.3"
  | "rate.4"
  | "rate.5"
  | "flag.pick"
  | "flag.reject"
  | "flag.unflag"
  | "flag.toggle"
  | "label.red"
  | "label.yellow"
  | "label.green"
  | "label.blue"
  | "label.purple"
  | "ui.toggleFilterBar"
  | "ui.toggleMirror"
  | "view.grid"
  | "view.viewer"
  | "view.compare"
  | "view.back"
  | "zoom.toggle"
  | "zoom.in"
  | "zoom.out"
  | "pair.toggleShown"
  | "pair.toggleCoupling"
  | "tag.chord"
  | "delete.pair"
  | "delete.rawOnly"
  | "delete.jpegOnly"
  | "action.moveCopy"
  | "commit.open"
  | "info.toggle"
  | "ui.toggleFilmstrip"
  | "ui.toggleFolderTree"
  | "ui.toggleShortcuts";

/** Logical grouping for the shortcuts cheat-sheet overlay. */
export type CommandCategory =
  | "Navigation"
  | "Rating & flags"
  | "Labels"
  | "Views"
  | "Zoom"
  | "Pairs"
  | "Actions"
  | "UI";

export interface CommandMeta {
  id: CommandId;
  title: string;
  /** Section the command appears under in the shortcuts cheat-sheet. */
  category: CommandCategory;
  /** Classification commands honour Caps Lock / Shift auto-advance. */
  classify?: boolean;
}

export const COMMANDS: CommandMeta[] = [
  { id: "nav.next", title: "Next photo", category: "Navigation" },
  { id: "nav.prev", title: "Previous photo", category: "Navigation" },
  { id: "nav.down", title: "Row down", category: "Navigation" },
  { id: "nav.up", title: "Row up", category: "Navigation" },
  { id: "nav.home", title: "First photo", category: "Navigation" },
  { id: "nav.end", title: "Last photo", category: "Navigation" },
  { id: "select.all", title: "Select all", category: "Navigation" },
  { id: "rate.0", title: "Clear stars", category: "Rating & flags", classify: true },
  { id: "rate.1", title: "1 star", category: "Rating & flags", classify: true },
  { id: "rate.2", title: "2 stars", category: "Rating & flags", classify: true },
  { id: "rate.3", title: "3 stars", category: "Rating & flags", classify: true },
  { id: "rate.4", title: "4 stars", category: "Rating & flags", classify: true },
  { id: "rate.5", title: "5 stars", category: "Rating & flags", classify: true },
  { id: "flag.pick", title: "Flag as pick", category: "Rating & flags", classify: true },
  { id: "flag.reject", title: "Flag as reject", category: "Rating & flags", classify: true },
  { id: "flag.unflag", title: "Remove flag", category: "Rating & flags", classify: true },
  { id: "flag.toggle", title: "Toggle pick flag", category: "Rating & flags", classify: true },
  { id: "label.red", title: "Red label", category: "Labels", classify: true },
  { id: "label.yellow", title: "Yellow label", category: "Labels", classify: true },
  { id: "label.green", title: "Green label", category: "Labels", classify: true },
  { id: "label.blue", title: "Blue label", category: "Labels", classify: true },
  { id: "label.purple", title: "Purple label", category: "Labels", classify: true },
  { id: "view.grid", title: "Grid view", category: "Views" },
  { id: "view.viewer", title: "Loupe view", category: "Views" },
  { id: "view.compare", title: "Compare view", category: "Views" },
  { id: "view.back", title: "Back to grid", category: "Views" },
  { id: "zoom.toggle", title: "Toggle zoom", category: "Zoom" },
  { id: "zoom.in", title: "Zoom in", category: "Zoom" },
  { id: "zoom.out", title: "Zoom out", category: "Zoom" },
  { id: "pair.toggleShown", title: "Show RAW ↔ JPEG half of pair", category: "Pairs" },
  { id: "pair.toggleCoupling", title: "Decouple / recouple pair", category: "Pairs" },
  { id: "tag.chord", title: "Task tag chord (then 1-9)", category: "Actions", classify: true },
  { id: "delete.pair", title: "Queue delete (whole pair)", category: "Actions", classify: true },
  { id: "delete.rawOnly", title: "Queue delete: RAW only", category: "Actions", classify: true },
  { id: "delete.jpegOnly", title: "Queue delete: JPEG only", category: "Actions", classify: true },
  { id: "action.moveCopy", title: "Queue move/copy to folder…", category: "Actions" },
  { id: "commit.open", title: "Review & commit pending actions", category: "Actions" },
  { id: "ui.toggleFilterBar", title: "Toggle filters panel", category: "UI" },
  { id: "ui.toggleMirror", title: "Toggle RAW+JPEG mirror mode", category: "UI" },
  { id: "info.toggle", title: "Show/hide camera metadata", category: "UI" },
  { id: "ui.toggleFilmstrip", title: "Show/hide filmstrip", category: "UI" },
  { id: "ui.toggleFolderTree", title: "Show/hide folder tree", category: "UI" },
  { id: "ui.toggleShortcuts", title: "Show/hide this shortcut list", category: "UI" },
];

/** Lightroom-compatible defaults (see docs/keymap research). */
export const DEFAULT_BINDINGS: Record<CommandId, string[]> = {
  "nav.next": ["arrowright"],
  "nav.prev": ["arrowleft"],
  "nav.down": ["arrowdown"],
  "nav.up": ["arrowup"],
  "nav.home": ["home"],
  "nav.end": ["end"],
  "select.all": ["ctrl+a"],
  "rate.0": ["0"],
  "rate.1": ["1"],
  "rate.2": ["2"],
  "rate.3": ["3"],
  "rate.4": ["4"],
  "rate.5": ["5"],
  "flag.pick": ["p"],
  "flag.reject": ["x"],
  "flag.unflag": ["u"],
  "flag.toggle": ["`"],
  "label.red": ["6"],
  "label.yellow": ["7"],
  "label.green": ["8"],
  "label.blue": ["9"],
  "label.purple": ["-"],
  "ui.toggleFilterBar": ["\\"],
  "ui.toggleMirror": ["m"],
  "view.grid": ["g"],
  "view.viewer": ["e", "enter"],
  "view.compare": ["c"],
  "view.back": ["escape"],
  "zoom.toggle": ["z", "space"],
  // Ctrl+= is the primary zoom-in; some keyboard layouts report the key as "+",
  // so bind that too. Ctrl+- zooms out.
  "zoom.in": ["ctrl+=", "ctrl++"],
  "zoom.out": ["ctrl+-"],
  "pair.toggleShown": ["j"],
  "pair.toggleCoupling": ["ctrl+j"],
  "tag.chord": ["t"],
  "delete.pair": ["delete"],
  "delete.rawOnly": ["alt+delete"],
  "delete.jpegOnly": ["shift+delete"],
  "action.moveCopy": ["v"],
  "commit.open": ["ctrl+enter"],
  "info.toggle": ["i"],
  "ui.toggleFilmstrip": ["f"],
  "ui.toggleFolderTree": ["d"],
  "ui.toggleShortcuts": ["?"],
};

const STORAGE_KEY = "cullant.keymap.v1";

export function loadOverrides(): Partial<Record<CommandId, string[]>> {
  try {
    return JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
  } catch {
    return {};
  }
}

export function saveOverrides(overrides: Partial<Record<CommandId, string[]>>) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(overrides));
}

export function effectiveBindings(
  overrides: Partial<Record<CommandId, string[]>>
): Map<string, CommandId> {
  const map = new Map<string, CommandId>();
  for (const cmd of COMMANDS) {
    const keys = overrides[cmd.id] ?? DEFAULT_BINDINGS[cmd.id];
    for (const key of keys) map.set(key, cmd.id);
  }
  return map;
}

/**
 * Normalize a KeyboardEvent to a binding string like "ctrl+shift+delete".
 * Lookup strategy: try WITH shift first (explicit shift bindings win), then
 * without it — for classification keys shift is the auto-advance inverter,
 * not part of the binding.
 */
export function normalizeKey(e: KeyboardEvent, includeShift = true): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("ctrl");
  if (e.altKey) parts.push("alt");
  if (includeShift && e.shiftKey) parts.push("shift");
  const key = e.key.toLowerCase();
  parts.push(key === " " ? "space" : key);
  return parts.join("+");
}
