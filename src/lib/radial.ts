/**
 * The radial menu's contents, as data.
 *
 * A slot is one sector of the ring. It holds either a single command, or a
 * GROUP whose members fan out past the rim while the finger keeps moving —
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
  "commit.open": "Review changes",
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

/** How a sector is drawn: an icon, a star count, or a colour swatch. */
export interface SlotFace {
  icon?: typeof Star;
  /** Drawn as that many stars, so a rating reads as a rating. */
  stars?: number;
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
    if (stars) return { stars };
    const label = LABEL_OF_COMMAND[slot.id];
    if (label) return { swatch: LABEL_COLORS[label] };
  }
  return { icon: slotIcon(slot) };
}

/**
 * The ring's geometry, in the units of a `RING_BOX`-wide square. The live menu
 * draws it at that size and the settings preview scales the same box down, so
 * the two can never disagree about proportions.
 */
export const RING_BOX = 250;
export const RING_INNER = 40;
export const RING_OUTER = 108;
export const RING_ICON = (RING_INNER + RING_OUTER) / 2;
/** Past this distance from the centre a group sector opens into its members.
 *  Deliberately well inside the drawn ring: having to travel past the edge of
 *  what you can see is not something anyone discovers. */
export const RING_DESCEND = 76;

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
 * An arc across sector `i` at radius `r`, centred in the sector. `span` is the
 * fraction of the sector's usable width it covers, so the same arc can be drawn
 * as a short tab or grown into a full-width bar.
 */
export function arcPath(
  i: number,
  n: number,
  r: number,
  box: number,
  rot = 0,
  span = 1,
): string {
  const step = (Math.PI * 2) / n;
  // Kept just inside the sector's own edges so neighbouring arcs never touch.
  const inset = Math.min(step * 0.12, 0.12);
  const width = Math.max(0.001, (step - inset * 2) * span);
  const a0 = i * step - Math.PI / 2 + rot - width / 2;
  const a1 = a0 + width;
  const c = box / 2;
  const p = (a: number) => `${c + r * Math.cos(a)} ${c + r * Math.sin(a)}`;
  const large = width > Math.PI ? 1 : 0;
  return `M ${p(a0)} A ${r} ${r} 0 ${large} 1 ${p(a1)}`;
}

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

/** The fan a group opens into, past the rim of the ring. */
export const FAN_R = RING_OUTER * 1.09 + 26;
/** Every fan member sits on a plate this wide, whatever it shows, so equal
 *  steps between centres are equal gaps between plates. A plate sized to its
 *  content made five stars crowd four while one star stood apart. */
export const FAN_CHIP = 34;
/** Arc distance between fan members' centres: a plate plus a clear gap. */
export const FAN_GAP = FAN_CHIP + 8;
/** At most this many members; past it, the last place opens the full list. */
export const FAN_MAX = 7;
/** Room kept between a fan member's centre and the window edge. */
export const FAN_MARGIN = 22;

/** SVG angle (0 to the right, clockwise) of the centre of sector `i` of `n`. */
export function sectorAngle(i: number, n: number, rot = 0): number {
  return i * ((Math.PI * 2) / n) - Math.PI / 2 + rot;
}

/**
 * Angles for a fan of `count` items on a circle of radius `r`, centred on angle
 * `centre` (SVG convention: 0 is to the right, growing clockwise) and `gap`
 * pixels apart along the arc.
 *
 * `fits` says whether an item at a given angle is fully on screen. A fan that
 * does not fit where it wants to be slides along the circle towards the side
 * with room and tightens its spacing, and only if neither is enough goes as
 * far as the far side of the ring — in a corner that is the only side left.
 * Null only when no angle and no spacing fits at all.
 */
export function fanAngles(
  count: number,
  r: number,
  centre: number,
  gap: number,
  fits: (angle: number) => boolean,
): number[] | null {
  if (count <= 0) return [];
  const SLIDE_STEP = 0.03;
  // Cheapest fix first: a short slide, then tighter spacing, and only then
  // the long way round.
  // Tightening stops short of the plates touching (see FAN_GAP).
  const passes: [squeeze: number, maxSlide: number][] = [
    [1, Math.PI / 3],
    [0.9, Math.PI / 3],
    [1, Math.PI],
    [0.9, Math.PI],
  ];
  for (const [squeeze, maxSlide] of passes) {
    const spread = ((count - 1) * gap * squeeze) / r;
    const at = (c: number) =>
      Array.from({ length: count }, (_, j) => c + (count === 1 ? 0 : (j / (count - 1) - 0.5) * spread));
    for (let d = 0; d <= maxSlide; d += SLIDE_STEP) {
      for (const c of d === 0 ? [centre] : [centre + d, centre - d]) {
        const angles = at(c);
        if (angles.every(fits)) return angles;
      }
    }
  }
  return null;
}
