/**
 * The radial menu's contents, as data.
 *
 * A slot is one sector of the ring. It holds either a single command, or a
 * GROUP whose members the ring descends into while the finger keeps moving —
 * ratings and colour labels have six and five values each, and spending six
 * sectors on stars would leave no room for anything else.
 *
 * One slot is always `more`, and it cannot be removed: it opens the full
 * command list, so no arrangement of the ring can leave a command unreachable.
 * It opens a LIST rather than a second ring on purpose — the unassigned set is
 * some forty commands, and a ring of forty sectors is not a menu.
 *
 * Slots serialize as short strings (`cmd:flag.pick`, `group:stars`, `more`) so a
 * stored layout stays readable and easy to heal against a changed command set.
 */

import { COMMANDS, type CommandId } from "./keyboard/keymap";

export type RadialGroupId = "stars" | "labels" | "tags";

export type RadialSlot =
  | { kind: "command"; id: CommandId }
  | { kind: "group"; id: RadialGroupId }
  | { kind: "more" };

export const RADIAL_GROUPS: { id: RadialGroupId; label: string }[] = [
  { id: "stars", label: "Star rating" },
  { id: "labels", label: "Color labels" },
  { id: "tags", label: "Tags" },
];

/** How many sectors the ring may be split into. Four is a plain cross; eight is
 *  as fine as a thumb can aim at speed. */
export const RADIAL_SECTOR_CHOICES = [4, 6, 8] as const;

/** Which mouse gesture opens the ring on the desktop. */
export const RADIAL_MOUSE_CHOICES = ["left", "right", "both"] as const;
export type RadialMouse = (typeof RADIAL_MOUSE_CHOICES)[number];

/**
 * Sector 0 is at the top and they run clockwise, so this list reads as the ring
 * does. The default puts the two one-flick verdicts on the sides where a thumb
 * reaches first, ratings on top, and everything else behind `more`.
 */
export const DEFAULT_RADIAL_SLOTS: RadialSlot[] = [
  { kind: "group", id: "stars" },
  { kind: "command", id: "flag.pick" },
  { kind: "more" },
  { kind: "command", id: "flag.unflag" },
  { kind: "command", id: "flag.reject" },
  { kind: "group", id: "labels" },
];

/** Commands worth putting on the ring. Navigation and view switching are left
 *  out: they are not what a hold-and-flick over a photo is for, and they stay
 *  reachable through `more`. */
const RING_EXCLUDED_CATEGORIES = new Set(["Navigation", "Views", "Zoom", "UI"]);

export const ASSIGNABLE_COMMANDS = COMMANDS.filter(
  (c) => !RING_EXCLUDED_CATEGORIES.has(c.category),
);

export function slotKey(slot: RadialSlot): string {
  return slot.kind === "more" ? "more" : `${slot.kind === "command" ? "cmd" : "group"}:${slot.id}`;
}

export function slotLabel(slot: RadialSlot): string {
  switch (slot.kind) {
    case "more":
      return "More…";
    case "group":
      return RADIAL_GROUPS.find((g) => g.id === slot.id)?.label ?? slot.id;
    case "command":
      return COMMANDS.find((c) => c.id === slot.id)?.title ?? slot.id;
  }
}

function parseSlot(key: string): RadialSlot | null {
  if (key === "more") return { kind: "more" };
  const [kind, id] = key.split(":", 2);
  if (kind === "group") {
    return RADIAL_GROUPS.some((g) => g.id === id) ? { kind: "group", id: id as RadialGroupId } : null;
  }
  if (kind === "cmd") {
    return COMMANDS.some((c) => c.id === id) ? { kind: "command", id: id as CommandId } : null;
  }
  return null;
}

/**
 * Read a stored layout back, healed against the current build: keys that no
 * longer resolve are dropped, `more` is forced back in if it went missing, and
 * the list is trimmed or padded to `sectors`. A layout can therefore never
 * arrive in a state that hides commands or leaves a hole in the ring.
 */
export function healSlots(raw: unknown, sectors: number): RadialSlot[] {
  const parsed = Array.isArray(raw)
    ? raw.filter((k): k is string => typeof k === "string").map(parseSlot).filter((s) => s !== null)
    : [];
  const slots: RadialSlot[] = parsed.length > 0 ? parsed : [...DEFAULT_RADIAL_SLOTS];
  // Trim before checking for `more`, or trimming could be what removes it.
  slots.length = Math.min(slots.length, sectors);
  if (!slots.some((s) => s.kind === "more")) {
    if (slots.length < sectors) slots.push({ kind: "more" });
    else slots[slots.length - 1] = { kind: "more" };
  }
  // Pad from the defaults, skipping what is already placed.
  for (const fill of DEFAULT_RADIAL_SLOTS) {
    if (slots.length >= sectors) break;
    if (!slots.some((s) => slotKey(s) === slotKey(fill))) slots.push(fill);
  }
  // Still short (a tiny default list against eight sectors): any spare command.
  for (const cmd of ASSIGNABLE_COMMANDS) {
    if (slots.length >= sectors) break;
    const slot: RadialSlot = { kind: "command", id: cmd.id };
    if (!slots.some((s) => slotKey(s) === slotKey(slot))) slots.push(slot);
  }
  return slots;
}
