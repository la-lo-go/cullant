export type ViewMode = "grid" | "viewer" | "compare";

/**
 * Hard zoom ceiling: 4x of 1:1 pixel scale. The effective MINIMUM is the
 * per-photo "fit" scale (whole image visible), which depends on the viewport
 * and is therefore computed inside ZoomImage, not here.
 */
export const MAX_SCALE = 4;

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
  /** Read-only keyboard cheat-sheet overlay open (toggled by `?`). */
  shortcutsOpen = $state(false);

  /**
   * Bumped by the keyboard zoom shortcut. The loupe's ZoomImage watches this
   * and runs its own fit-aware toggle (the store can't compute the per-photo
   * fit scale), so Z/Space match the double-tap behaviour exactly.
   */
  zoomToggleNonce = $state(0);

  /**
   * Bumped by the Ctrl+= / Ctrl+- zoom step shortcuts. Like the toggle nonce,
   * the loupe's ZoomImage watches it and applies a centred zoom step, because
   * only the component knows the per-photo fit scale. `zoomStepDir` carries the
   * direction of the pending step (+1 in, -1 out) and is a plain field: it is
   * always set before the nonce is bumped, so the watcher reads it consistently.
   */
  zoomStepNonce = $state(0);
  zoomStepDir = 0;

  requestZoomToggle() {
    this.zoomToggleNonce++;
  }

  requestZoomStep(dir: number) {
    this.zoomStepDir = dir;
    this.zoomStepNonce++;
  }

  resetZoom() {
    this.zoomed = false;
    this.scale = 1;
    this.cx = 0.5;
    this.cy = 0.5;
  }
}

export const view = new ViewStore();
