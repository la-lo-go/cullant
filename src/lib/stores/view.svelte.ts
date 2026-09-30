export type ViewMode = "grid" | "viewer" | "compare" | "survey";

/**
 * Base zoom ceiling: 6x of 1:1 pixel scale. ZoomImage raises this limit to
 * at least twice the fit scale, so small images can grow from their visible
 * size. ZoomImage also computes the minimum scale from the viewport.
 */
export const MAX_SCALE = 6;

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

  /**
   * `performance.now()` of the last time the loupe was opened by a TAP in the
   * grid. On touch, users habitually double-tap to open a photo: the first tap
   * opens the loupe, the second lands on the freshly mounted ZoomImage and would
   * otherwise be read as a double-tap-to-zoom. ZoomImage swallows its zoom-toggle
   * gestures for a brief window after this timestamp, so opening always lands at
   * fit; a deliberate double-tap-to-zoom still works once the window lapses.
   */
  openedFromGridAt = $state(0);
  /** Read-only keyboard cheat-sheet overlay open (toggled by `?`). */
  shortcutsOpen = $state(false);

  /** Which settings sub-panel is open. Lives here rather than inside the dialog
   *  so the app's single back/Escape ladder in +page.svelte can step out of it
   *  before it closes the dialog itself. */
  settingsPanel = $state<string | null>(null);

  /** The explanation currently on screen, wherever it was opened from: the
   *  settings rows, the view panel, anywhere an InfoTip sits. One at a time by
   *  construction, and one place for the back ladder to dismiss. `x`/`y` are the
   *  icon's viewport position, which is all the overlay needs to anchor itself
   *  on a wide screen (on a narrow one it is a full-width sheet). */
  infoTip = $state<{ title: string; text: string; x: number; y: number } | null>(null);

  /** Drop the settings dialog back to its top level. Called when it opens and
   *  when it closes, so a sub-panel left open never greets the next visit. */
  resetSettingsNav() {
    this.settingsPanel = null;
    this.infoTip = null;
  }
  /** Loupe/compare only: hides the top toolbar and the touch action bar so the
   *  photo gets the whole screen. Reset to false whenever the grid comes back
   *  (see the effect below) — it has no meaning there and must never linger. */
  fullscreen = $state(false);

  toggleFullscreen() {
    this.fullscreen = !this.fullscreen;
  }

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

  /** Mark that the loupe is being opened by a tap in the grid (touch). */
  markOpenedFromGrid() {
    this.openedFromGridAt = performance.now();
  }

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

$effect.root(() => {
  // Fullscreen is a loupe/compare-only affordance; back to the grid always
  // drops it, so the toolbar is never left hidden there and re-entering the
  // loupe next time starts un-fullscreened by default.
  $effect(() => {
    if (view.mode === "grid" && view.fullscreen) view.fullscreen = false;
  });
});
