/**
 * Keeping a floating panel on screen.
 *
 * Every popover in the app hangs off a toolbar button, and the toolbar wraps on
 * narrow screens, so a panel can be anchored anywhere horizontally. These two
 * helpers shift it back inside the margins and cap its height, using the same
 * gutter the CSS max-width reserves so the clamp and the width cap can never
 * disagree: the shared dialog edge margin (widened on narrow portrait phones)
 * plus the safe-area insets, read back from the root.
 */

/** Smallest height worth clamping to. Below this a panel is unusable anyway, so
 *  it is allowed to overflow rather than collapse to nothing. */
const MIN_HEIGHT = 120;

/** Gap left below the panel, on top of the bottom inset, so a strip of backdrop
 *  stays tappable to dismiss it. */
const BOTTOM_GAP = 24;

/**
 * Shift `el` fully on screen and cap its height to what is available.
 *
 * Cancels any previous offset first, then pushes left if the right edge
 * overflows, then pushes right if that took the left edge out. Past the height
 * cap the panel scrolls internally, which it must: a filter panel with every
 * facet showing is far taller than a phone screen.
 */
export function clampToViewport(el: HTMLElement | null) {
  if (!el) return;
  el.style.transform = "";
  const cs = getComputedStyle(document.documentElement);
  const px = (name: string) => parseFloat(cs.getPropertyValue(name)) || 0;
  const edge = px("--dialog-edge-margin") || 12;
  const marginL = edge + px("--inset-left");
  const marginR = edge + px("--inset-right");
  const rect = el.getBoundingClientRect();
  let dx = 0;
  if (rect.right > window.innerWidth - marginR) dx = window.innerWidth - marginR - rect.right;
  if (rect.left + dx < marginL) dx = marginL - rect.left;
  if (dx) el.style.transform = `translateX(${dx}px)`;

  el.style.maxHeight = "";
  const top = Math.max(el.getBoundingClientRect().top, px("--inset-top") + edge);
  const available = window.innerHeight - (edge + px("--inset-bottom") + BOTTOM_GAP) - top;
  if (el.offsetHeight > available) el.style.maxHeight = `${Math.max(available, MIN_HEIGHT)}px`;
}

/**
 * Clamp now and on every resize, returning the teardown so a caller can hand it
 * straight back from an `$effect`:
 *
 * ```svelte
 * $effect(() => keepClamped(() => panelEl));
 * ```
 *
 * `el` is a getter rather than the element because the effect runs before the
 * binding settles on the first pass.
 */
export function keepClamped(el: () => HTMLElement | null): () => void {
  clampToViewport(el());
  // Throttle to one clamp per frame: the raw resize event fires far faster than
  // a repaint, and each clamp forces synchronous layout.
  let raf = 0;
  const onResize = () => {
    if (raf) return;
    raf = requestAnimationFrame(() => {
      raf = 0;
      clampToViewport(el());
    });
  };
  window.addEventListener("resize", onResize);
  return () => {
    if (raf) cancelAnimationFrame(raf);
    window.removeEventListener("resize", onResize);
  };
}
