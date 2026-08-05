/**
 * The radial menu's contents, as data.
 *
 * A slot is one sector of the ring. It holds either a single command, or a
 * GROUP whose members the ring descends into while the finger keeps moving —
 * ratings and colour labels have six and five values each, and spending six
 * sectors on stars would leave no room for anything else.
 *
 * The `more` slot opens the full command list. It is an ordinary slot like any
 * other — put it on the ring or leave it off. It opens a LIST rather than a
 * second ring on purpose: the unassigned set is some forty commands, and a ring
 * of forty sectors is not a menu.
 *
 * Slots serialize as short strings (`cmd:flag.pick`, `group:stars`, `more`) so a
 * stored layout stays readable and easy to heal against a changed command set.
 */

import { COMMANDS, type CommandId } from "./keyboard/keymap";
import { LABEL_COLORS } from "./labels";

import Circle from "@lucide/svelte/icons/circle";
import Flag from "@lucide/svelte/icons/flag";
import FlagOff from "@lucide/svelte/icons/flag-off";
import Ellipsis from "@lucide/svelte/icons/ellipsis";
import FolderInput from "@lucide/svelte/icons/folder-input";
import Link2 from "@lucide/svelte/icons/link-2";
import Palette from "@lucide/svelte/icons/palette";
import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
import RotateCw from "@lucide/svelte/icons/rotate-cw";
import Scissors from "@lucide/svelte/icons/scissors";
import SplitSquareHorizontal from "@lucide/svelte/icons/square-split-horizontal";
import Star from "@lucide/svelte/icons/star";
import StarOff from "@lucide/svelte/icons/star-off";
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

/** Quarter turns the whole ring can be rotated by, so the first sector can be
 *  put where a given hand actually reaches. */
export const RADIAL_ROTATION_CHOICES = [0, 90, 180, 270] as const;

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
  // The three flag states as one family: raised, struck through, gone.
  "flag.pick": Flag,
  "flag.reject": X,
  "flag.unflag": FlagOff,
  "flag.toggle": Flag,
  "rate.0": StarOff,
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

/**
 * What a sector is called ON THE RING, as opposed to in the settings list.
 *
 * The registry's titles are written for a cheat sheet, where "Review & commit
 * pending actions" is exactly right. A sector is a wedge a few centimetres
 * across, read in the half second before the finger lifts, so the same command
 * has to answer in one or two words or the label is noise the eye skips anyway.
 */
const SHORT_LABELS: Partial<Record<CommandId, string>> = {
  "rate.0": "No stars",
  "rate.1": "★",
  "rate.2": "★★",
  "rate.3": "★★★",
  "rate.4": "★★★★",
  "rate.5": "★★★★★",
  "flag.pick": "Pick",
  "flag.reject": "Reject",
  "flag.unflag": "Unflag",
  "flag.toggle": "Toggle pick",
  "label.red": "Red",
  "label.yellow": "Yellow",
  "label.green": "Green",
  "label.blue": "Blue",
  "label.purple": "Purple",
  "pair.toggleShown": "Flip half",
  "pair.toggleCoupling": "Decouple",
  "tag.chord": "Tag",
  "delete.pair": "Delete",
  "delete.rawOnly": "RAW only",
  "delete.jpegOnly": "JPEG only",
  "edit.rotateLeft": "Rotate ↺",
  "edit.rotateRight": "Rotate ↻",
  "action.moveCopy": "Move/copy",
  "commit.open": "Commit",
};

const SHORT_GROUPS: Record<RadialGroupId, string> = {
  stars: "Rating",
  labels: "Labels",
  tags: "Tags",
};

export function slotShortLabel(slot: RadialSlot): string {
  if (slot.kind === "more") return "More";
  if (slot.kind === "group") return SHORT_GROUPS[slot.id];
  return SHORT_LABELS[slot.id] ?? slotLabel(slot);
}

export function slotIcon(slot: RadialSlot): typeof Star {
  if (slot.kind === "more") return Ellipsis;
  if (slot.kind === "group") return GROUP_ICONS[slot.id];
  return COMMAND_ICONS[slot.id] ?? Circle;
}

/** How a sector is drawn: an icon, a short glyph, or a colour swatch. */
export interface SlotFace {
  icon?: typeof Star;
  glyph?: string;
  swatch?: string;
}

const RATE_STARS: Partial<Record<CommandId, number>> = {
  "rate.1": 1,
  "rate.2": 2,
  "rate.3": 3,
  "rate.4": 4,
  "rate.5": 5,
};

const LABEL_OF_COMMAND: Partial<Record<CommandId, string>> = {
  "label.red": "Red",
  "label.yellow": "Yellow",
  "label.green": "Green",
  "label.blue": "Blue",
  "label.purple": "Purple",
};

/**
 * A rating or a colour label is drawn AS the rating or the colour, wherever it
 * appears. A sector set to "Yellow label" showed a generic palette while the
 * labels group two levels in showed actual yellow — the same command wearing two
 * faces depending on how you got to it.
 */
export function slotFace(slot: RadialSlot): SlotFace {
  if (slot.kind === "command") {
    const stars = RATE_STARS[slot.id];
    if (stars) return { glyph: "★".repeat(stars) };
    const label = LABEL_OF_COMMAND[slot.id];
    if (label) return { swatch: LABEL_COLORS[label] };
  }
  return { icon: slotIcon(slot) };
}

/**
 * Sector `i` of `n` as an SVG wedge, centred on straight up and running
 * clockwise, drawn in a box `2 * rOuter` across. Shared by the live ring and the
 * settings preview so the two can never disagree about the shape.
 */
export function wedgePath(
  i: number,
  n: number,
  rInner: number,
  rOuter: number,
  box: number,
  rot = 0,
): string {
  const step = (Math.PI * 2) / n;
  const a0 = i * step - step / 2 - Math.PI / 2 + rot;
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

/**
 * The arc across sector `i` at radius `r` — the line the finger has to reach to
 * open a group. Drawn so the pull a group needs is something you can see coming
 * rather than something you have to already know about.
 */
export function arcPath(i: number, n: number, r: number, box: number, rot = 0): string {
  const step = (Math.PI * 2) / n;
  // Kept just inside the sector's own edges so neighbouring arcs never touch.
  const inset = Math.min(step * 0.12, 0.12);
  const a0 = i * step - step / 2 - Math.PI / 2 + inset + rot;
  const a1 = a0 + step - inset * 2;
  const c = box / 2;
  const p = (a: number) => `${c + r * Math.cos(a)} ${c + r * Math.sin(a)}`;
  const large = step > Math.PI ? 1 : 0;
  return `M ${p(a0)} A ${r} ${r} 0 ${large} 1 ${p(a1)}`;
}

/** Centre point of sector `i` at radius `r`, in the same box. */
export function sectorPoint(
  i: number,
  n: number,
  r: number,
  box: number,
  rot = 0,
): { x: number; y: number } {
  const a = i * ((Math.PI * 2) / n) - Math.PI / 2 + rot;
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
 * longer resolve are dropped, duplicates collapse, and the length is held inside
 * the sector bounds.
 *
 * `more` is an ordinary slot here — it can be removed like any other. A ring
 * without it can leave a command off the ring entirely, which is the user's call
 * to make: the keyboard still reaches everything, and forcing a sector nobody
 * wants is worse than a ring that does exactly what it was set up to do.
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
  for (const fill of DEFAULT_RADIAL_SLOTS) {
    if (slots.length >= RADIAL_MIN_SECTORS) break;
    if (!slots.some((s) => slotKey(s) === slotKey(fill))) slots.push(fill);
  }
  return slots;
}
