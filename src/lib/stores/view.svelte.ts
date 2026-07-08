export type ViewMode = "grid" | "viewer" | "compare";

/**
 * Zoom/pan state shared across photos and across viewer/compare: expressed in
 * RELATIVE image coordinates (cx, cy in 0..1), so stepping to the next photo
 * of a burst keeps the same eye/detail under the cursor — the focus-check loop.
 */
class ViewStore {
  mode = $state<ViewMode>("grid");
  zoomed = $state(false);
  cx = $state(0.5);
  cy = $state(0.5);

  toggleZoom(atX?: number, atY?: number) {
    if (!this.zoomed && atX !== undefined && atY !== undefined) {
      this.cx = atX;
      this.cy = atY;
    }
    this.zoomed = !this.zoomed;
  }

  resetZoom() {
    this.zoomed = false;
    this.cx = 0.5;
    this.cy = 0.5;
  }
}

export const view = new ViewStore();
