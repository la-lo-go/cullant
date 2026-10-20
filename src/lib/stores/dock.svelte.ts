export type Side = "top" | "right" | "bottom" | "left";
export type DockPanel = "folderTree" | "filmstrip" | "actionBar";

export const DOCK_SIDES: Record<DockPanel, readonly Side[]> = {
  folderTree: ["left", "right"],
  filmstrip: ["top", "right", "bottom", "left"],
  actionBar: ["top", "right", "bottom", "left"],
};

export const DOCK_DEFAULTS: Record<DockPanel, Side> = {
  folderTree: "left",
  filmstrip: "bottom",
  actionBar: "bottom",
};

export const DOCK_LABELS: Record<DockPanel, string> = {
  folderTree: "Folder tree",
  filmstrip: "Filmstrip",
  actionBar: "Action bar",
};

export const SIDE_LABELS: Record<Side, string> = {
  top: "Top",
  right: "Right",
  bottom: "Bottom",
  left: "Left",
};

/** True when the panel is a column (docked left or right). */
export function isVertical(side: Side): boolean {
  return side === "left" || side === "right";
}

export interface DockDrag {
  panel: DockPanel;
  /** Re-read on every pointer move. */
  region: DOMRect;
  /** Pointer position, client coordinates. */
  x: number;
  y: number;
  candidate: Side | null;
  /** Size in px of the panel if it is docked at that side. */
  thickness: (side: Side) => number;
}

/** Compass geometry in px. `DockGuides` draws from the same constants that `pickSide` tests. */
export const COMPASS_BUTTON = 36;
export const COMPASS_HUB = 20;
/** Distance from the compass centre to the centre of each button. */
export const COMPASS_OFFSET = 40;
const EDGE_ZONE_MAX = 64;
const EDGE_ZONE_RATIO = 0.2;

const COMPASS_DIRECTION: Record<Side, readonly [number, number]> = {
  top: [0, -1],
  right: [1, 0],
  bottom: [0, 1],
  left: [-1, 0],
};

function edgeDistance(region: DOMRect, side: Side, x: number, y: number): number {
  switch (side) {
    case "top":
      return y - region.top;
    case "bottom":
      return region.bottom - y;
    case "left":
      return x - region.left;
    case "right":
      return region.right - x;
  }
}

function edgeZone(region: DOMRect, side: Side): number {
  const span = isVertical(side) ? region.width : region.height;
  return Math.min(EDGE_ZONE_MAX, span * EDGE_ZONE_RATIO);
}

function compassHit(region: DOMRect, x: number, y: number, allowed: readonly Side[]): Side | null {
  const cx = region.left + region.width / 2;
  const cy = region.top + region.height / 2;
  const half = COMPASS_BUTTON / 2;
  return (
    allowed.find((side) => {
      const [dx, dy] = COMPASS_DIRECTION[side];
      return (
        Math.abs(x - (cx + dx * COMPASS_OFFSET)) <= half &&
        Math.abs(y - (cy + dy * COMPASS_OFFSET)) <= half
      );
    }) ?? null
  );
}

function edgeHit(region: DOMRect, x: number, y: number, allowed: readonly Side[]): Side | null {
  let best: Side | null = null;
  let bestDistance = Infinity;
  for (const side of allowed) {
    const d = edgeDistance(region, side, x, y);
    if (d <= edgeZone(region, side) && d < bestDistance) {
      best = side;
      bestDistance = d;
    }
  }
  return best;
}

/** Dock side under the pointer: a compass button first, then the nearest edge zone. */
export function pickSide(
  region: DOMRect,
  x: number,
  y: number,
  allowed: readonly Side[]
): Side | null {
  const inside = x >= region.left && x <= region.right && y >= region.top && y <= region.bottom;
  if (!inside) return null;
  return compassHit(region, x, y, allowed) ?? edgeHit(region, x, y, allowed);
}

/** Client-coordinate rectangle that the panel fills when docked at `side`. */
export function previewRect(region: DOMRect, side: Side, thickness: number) {
  const { left, top, width, height } = region;
  switch (side) {
    case "top":
      return { left, top, width, height: thickness };
    case "bottom":
      return { left, top: top + height - thickness, width, height: thickness };
    case "left":
      return { left, top, width: thickness, height };
    case "right":
      return { left: left + width - thickness, top, width: thickness, height };
  }
}

const STORAGE_KEY = "cullant.dock.v1";
const PANELS = Object.keys(DOCK_DEFAULTS) as DockPanel[];

function load(): Record<DockPanel, Side> {
  const result = { ...DOCK_DEFAULTS };
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    const parsed = raw === null ? null : JSON.parse(raw);
    if (parsed && typeof parsed === "object") {
      for (const panel of PANELS) {
        const value = parsed[panel];
        if (DOCK_SIDES[panel].includes(value)) result[panel] = value;
      }
    }
  } catch {
    // a corrupt value falls back to the defaults
  }
  return result;
}

class DockStore {
  folderTree = $state<Side>(DOCK_DEFAULTS.folderTree);
  filmstrip = $state<Side>(DOCK_DEFAULTS.filmstrip);
  actionBar = $state<Side>(DOCK_DEFAULTS.actionBar);
  /** Set while the user drags a grip. */
  drag = $state<DockDrag | null>(null);

  constructor() {
    Object.assign(this, load());
  }

  set(panel: DockPanel, side: Side) {
    if (!DOCK_SIDES[panel].includes(side)) return;
    this[panel] = side;
    try {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({
          folderTree: this.folderTree,
          filmstrip: this.filmstrip,
          actionBar: this.actionBar,
        })
      );
    } catch {
      // persistence is best-effort
    }
  }
}

export const dock = new DockStore();
