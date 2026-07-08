export type ViewMode = "grid" | "viewer" | "compare";

export const MIN_SCALE = 0.1;
export const MAX_SCALE = 4;

export function clampScale(s: number): number {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, s));
}

/**
 * Zoom/pan state shared across photos and across viewer/compare: expressed in
 * RELATIVE image coordinates (cx, cy in 0..1), so stepping to the next photo
 * of a burst keeps the same eye/detail under the cursor — the focus-check loop.
 * `scale` is relative to source pixels: 1.0 = one image px per CSS px.
 */
class ViewStore {
  mode = $state<ViewMode>("grid");
  zoomed = $state(false);
  scale = $state(1);
  cx = $state(0.5);
  cy = $state(0.5);
  /** Camera-metadata panel open in the loupe. */
  infoOpen = $state(false);

  toggleZoom(atX?: number, atY?: number) {
    if (!this.zoomed) {
      if (atX !== undefined && atY !== undefined) {
        this.cx = atX;
        this.cy = atY;
      }
      this.scale = 1;
    }
    this.zoomed = !this.zoomed;
  }

  resetZoom() {
    this.zoomed = false;
    this.scale = 1;
    this.cx = 0.5;
    this.cy = 0.5;
  }
}

export const view = new ViewStore();
