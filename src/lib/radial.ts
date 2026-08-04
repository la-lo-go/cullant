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

import Check from "@lucide/svelte/icons/check";
import Circle from "@lucide/svelte/icons/circle";
import Ellipsis from "@lucide/svelte/icons/ellipsis";
import FolderInput from "@lucide/svelte/icons/folder-input";
import Link2 from "@lucide/svelte/icons/link-2";
import Palette from "@lucide/svelte/icons/palette";
import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
import RotateCw from "@lucide/svelte/icons/rotate-cw";
import Scissors from "@lucide/svelte/icons/scissors";
import SplitSquareHorizontal from "@lucide/svelte/icons/square-split-horizontal";
import Star from "@lucide/svelte/icons/star";
import Tag from "@lucide/svelte/icons/tag";
import Trash2 from "@lucide/svelte/icons/trash-2";
import UploadCloud from "@lucide/svelte/icons/upload-cloud";
import X from "@lucide/svelte/icons/x";

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

/** How many sectors the ring may be split into. Below two there is no choice to
 *  make; above ten a sector is thinner than a thumb can aim at while moving. */
export const RADIAL_MIN_SECTORS = 2;
export const RADIAL_MAX_SECTORS = 10;

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

/** The ring shows icons, not words — a label at a glance is slower to read than
 *  a shape, and a sector is not wide enough for "Queue delete: JPEG only". The
 *  name appears only for the sector currently under the finger. */
const COMMAND_ICONS: Partial<Record<CommandId, typeof Star>> = {
  "flag.pick": Check,
  "flag.reject": X,
  "flag.unflag": Circle,
  "flag.toggle": Check,
  "rate.0": Star,
  "rate.1": Star,
  "rate.2": Star,
  "rate.3": Star,
  "rate.4": Star,
  "rate.5": Star,
  "label.red": Palette,
  "label.yellow": Palette,
  "label.green": Palette,
  "label.blue": Palette,
  "label.purple": Palette,
  "delete.pair": Trash2,
  "delete.rawOnly": Trash2,
  "delete.jpegOnly": Trash2,
  "edit.rotateLeft": RotateCcw,
  "edit.rotateRight": RotateCw,
  "action.moveCopy": FolderInput,
  "pair.toggleCoupling": Scissors,
  "pair.toggleShown": SplitSquareHorizontal,
  "commit.open": UploadCloud,
  "tag.chord": Tag,
};

const GROUP_ICONS: Record<RadialGroupId, typeof Star> = {
  stars: Star,
  labels: Palette,
  tags: Tag,
};

export function slotIcon(slot: RadialSlot): typeof Star {
  if (slot.kind === "more") return Ellipsis;
  if (slot.kind === "group") return GROUP_ICONS[slot.id];
  return COMMAND_ICONS[slot.id] ?? Circle;
}

/**
 * Sector `i` of `n` as an SVG wedge, centred on straight up and running
 * clockwise, drawn in a box `2 * rOuter` across. Shared by the live ring and the
 * settings preview so the two can never disagree about the shape.
 */
export function wedgePath(i: number, n: number, rInner: number, rOuter: number, box: number): string {
  const step = (Math.PI * 2) / n;
  const a0 = i * step - step / 2 - Math.PI / 2;
  const a1 = a0 + step;
  const c = box / 2;
  const p = (r: number, a: number) => `${c + r * Math.cos(a)} ${c + r * Math.sin(a)}`;
  const large = step > Math.PI ? 1 : 0;
  if (n === 1) {
    // A single sector is a full annulus; an arc from a point to itself draws
    // nothing, so it needs two half-circles.
    return (
      `M ${c - rOuter} ${c} A ${rOuter} ${rOuter} 0 1 1 ${c + rOuter} ${c} ` +
      `A ${rOuter} ${rOuter} 0 1 1 ${c - rOuter} ${c} Z ` +
      `M ${c - rInner} ${c} A ${rInner} ${rInner} 0 1 0 ${c + rInner} ${c} ` +
      `A ${rInner} ${rInner} 0 1 0 ${c - rInner} ${c} Z`
    );
  }
  return (
    `M ${p(rInner, a0)} A ${rInner} ${rInner} 0 ${large} 1 ${p(rInner, a1)} ` +
    `L ${p(rOuter, a1)} A ${rOuter} ${rOuter} 0 ${large} 0 ${p(rOuter, a0)} Z`
  );
}

/** Centre point of sector `i` at radius `r`, in the same box. */
export function sectorPoint(i: number, n: number, r: number, box: number): { x: number; y: number } {
  const a = i * ((Math.PI * 2) / n) - Math.PI / 2;
  const c = box / 2;
  return { x: c + r * Math.cos(a), y: c + r * Math.sin(a) };
}

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
 * longer resolve are dropped, duplicates collapse, `more` is forced back in if
 * it went missing, and the length is held inside the sector bounds. A layout can
 * therefore never arrive in a state that hides commands or leaves a hole.
 */
export function healSlots(raw: unknown): RadialSlot[] {
  const parsed = Array.isArray(raw)
    ? raw.filter((k): k is string => typeof k === "string").map(parseSlot).filter((s) => s !== null)
    : [];
  const seen = new Set<string>();
  const slots: RadialSlot[] = [];
  for (const slot of parsed.length > 0 ? parsed : DEFAULT_RADIAL_SLOTS) {
    const key = slotKey(slot);
    if (seen.has(key) || slots.length >= RADIAL_MAX_SECTORS) continue;
    seen.add(key);
    slots.push(slot);
  }
  if (!seen.has("more")) {
    // Replacing the last sector rather than growing past the ceiling: `more` is
    // the one slot that has to be there.
    if (slots.length >= RADIAL_MAX_SECTORS) slots[slots.length - 1] = { kind: "more" };
    else slots.push({ kind: "more" });
  }
  for (const fill of DEFAULT_RADIAL_SLOTS) {
    if (slots.length >= RADIAL_MIN_SECTORS) break;
    if (!slots.some((s) => slotKey(s) === slotKey(fill))) slots.push(fill);
  }
  return slots;
}
