export type CommandId =
  | "nav.next"
  | "nav.prev"
  | "nav.down"
  | "nav.up"
  | "nav.home"
  | "nav.end"
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
  | "zoom.toggle"
  | "pair.toggleShown"
  | "pair.toggleCoupling";

export interface CommandMeta {
  id: CommandId;
  title: string;
  /** Classification commands honour Caps Lock / Shift auto-advance. */
  classify?: boolean;
}

export const COMMANDS: CommandMeta[] = [
  { id: "nav.next", title: "Next photo" },
  { id: "nav.prev", title: "Previous photo" },
  { id: "nav.down", title: "Row down" },
  { id: "nav.up", title: "Row up" },
  { id: "nav.home", title: "First photo" },
  { id: "nav.end", title: "Last photo" },
  { id: "rate.0", title: "Clear stars", classify: true },
  { id: "rate.1", title: "1 star", classify: true },
  { id: "rate.2", title: "2 stars", classify: true },
  { id: "rate.3", title: "3 stars", classify: true },
  { id: "rate.4", title: "4 stars", classify: true },
  { id: "rate.5", title: "5 stars", classify: true },
  { id: "flag.pick", title: "Flag as pick", classify: true },
  { id: "flag.reject", title: "Flag as reject", classify: true },
  { id: "flag.unflag", title: "Remove flag", classify: true },
  { id: "flag.toggle", title: "Toggle pick flag", classify: true },
  { id: "label.red", title: "Red label", classify: true },
  { id: "label.yellow", title: "Yellow label", classify: true },
  { id: "label.green", title: "Green label", classify: true },
  { id: "label.blue", title: "Blue label", classify: true },
  { id: "label.purple", title: "Purple label", classify: true },
  { id: "ui.toggleFilterBar", title: "Show/hide filter bar" },
  { id: "ui.toggleMirror", title: "Toggle RAW+JPEG mirror mode" },
  { id: "view.grid", title: "Grid view" },
  { id: "view.viewer", title: "Loupe view" },
  { id: "view.compare", title: "Compare view" },
  { id: "zoom.toggle", title: "Toggle 100% zoom" },
  { id: "pair.toggleShown", title: "Show RAW ↔ JPEG half of pair" },
  { id: "pair.toggleCoupling", title: "Decouple / recouple pair" },
];

/** Lightroom-compatible defaults (see docs/keymap research). */
export const DEFAULT_BINDINGS: Record<CommandId, string[]> = {
  "nav.next": ["arrowright"],
  "nav.prev": ["arrowleft"],
  "nav.down": ["arrowdown"],
  "nav.up": ["arrowup"],
  "nav.home": ["home"],
  "nav.end": ["end"],
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
  "zoom.toggle": ["z", "space"],
  "pair.toggleShown": ["j"],
  "pair.toggleCoupling": ["ctrl+j"],
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

/** Normalize a KeyboardEvent to a binding string like "ctrl+shift+p". */
export function normalizeKey(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("ctrl");
  if (e.altKey) parts.push("alt");
  // Shift is intentionally NOT part of classification bindings (it's the
  // auto-advance inverter), but is kept for explicit shift+ bindings.
  const key = e.key.toLowerCase();
  parts.push(key === " " ? "space" : key);
  return parts.join("+");
}
