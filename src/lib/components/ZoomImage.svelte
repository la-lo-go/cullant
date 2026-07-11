<script lang="ts">
  import { untrack } from "svelte";
  import { previewUrl, thumbUrl, cullantUrl, type ItemLite } from "../api";
  import { view, MAX_SCALE } from "../stores/view.svelte";
  import { session } from "../stores/session.svelte";
  import { settings } from "../stores/settings.svelte";

  let { item, standalone = false }: { item: ItemLite; standalone?: boolean } = $props();

  let frame = $state<HTMLDivElement | null>(null);
  let frameW = $state(0);
  let frameH = $state(0);
  // Natural = rendered (orientation-corrected) pixel dimensions of each source.
  let previewNaturalW = $state(0);
  let previewNaturalH = $state(0);
  let fullNaturalW = $state(0);
  let fullNaturalH = $state(0);
  let dragging = false;
  let lastX = 0;
  let lastY = 0;
  let wheelAccum = 0;

  // Active touch points (by pointerId), plus pinch/tap/swipe gesture state.
  // Only touch pointers use these; mouse/pen keep the drag-to-pan behavior.
  const pointers = new Map<number, { x: number; y: number }>();
  let pinchStartDist = 0;
  let pinchStartScale = 1;
  let pinchAnchor = { x: 0.5, y: 0.5 };
  let pinchedThisGesture = false;
  let swipeStartX = 0;
  let swipeStartY = 0;
  let lastTapTime = 0;
  let lastTapX = 0;
  let lastTapY = 0;
  let lastToggleTime = 0;

  const WHEEL_ZOOM = 1.15;
  const WHEEL_NAV_THRESHOLD = 50;
  const SWIPE_NAV_THRESHOLD = 55;
  /** Left/right margin width (as a fraction of the frame) that pages photos on
   *  a tap in the fit view, e-reader style. Kept narrow so the center is free
   *  for the double-tap zoom. */
  const EDGE_TAP_FRAC = 0.18;
  // A tap must land within this radius of its own start to count. It's kept
  // generous (not pixel-tight) because the FIRST tap of a zoom-OUT double-tap
  // happens on a zoomed image where one-finger pan is armed, so the finger
  // tends to drift a little; too tight a slop drops that tap and the double-tap
  // never completes (the old bug: double-tap could only ever zoom in). Real
  // pans move far more than this, so they're still never mistaken for taps.
  const TAP_SLOP = 18;
  /** Magnification (× fit) the double-tap / Z-key toggle jumps to from fit. */
  const DOUBLE_TAP_MAG = 1.5;
  const DOUBLE_TAP_MS = 300;
  /** Max spacing between the two taps of a double-tap (finger-down to down). */
  const DOUBLE_TAP_DIST = 44;
  /** Scales within this of `fit` count as "at fit" (i.e. not zoomed in). */
  const ZOOM_EPS = 1e-3;
  /** Collapse two zoom toggles fired within this window into one, so a manual
   *  touch double-tap and any browser-synthesized dblclick can't both fire. */
  const TOGGLE_GUARD_MS = 250;
  /** Damping exponent for pinching below fit — rubber-band resistance. */
  const RUBBER = 0.4;

  const clamp01 = (v: number) => Math.min(1, Math.max(0, v));

  // Standalone panes (compare) keep their own non-persistent zoom state;
  // the loupe uses the shared store so zoom survives stepping through a burst.
  let sZoomed = $state(false);
  let sScale = $state(1);
  let sCx = $state(0.5);
  let sCy = $state(0.5);

  const local = {
    get zoomed() {
      return sZoomed;
    },
    set zoomed(v: boolean) {
      sZoomed = v;
    },
    get scale() {
      return sScale;
    },
    set scale(v: number) {
      sScale = v;
    },
    get cx() {
      return sCx;
    },
    set cx(v: number) {
      sCx = v;
    },
    get cy() {
      return sCy;
    },
    set cy(v: number) {
      sCy = v;
    },
  };
  const z = $derived(standalone ? local : view);

  // Per-item source dimensions must not leak across photos.
  $effect(() => {
    void item.id;
    previewNaturalW = 0;
    previewNaturalH = 0;
    fullNaturalW = 0;
    fullNaturalH = 0;
  });

  // Reset to the fit view whenever the item changes — both the compare pane's
  // local state and the shared loupe zoom, so stepping to another photo always
  // starts unzoomed.
  $effect(() => {
    void item.id;
    // Any pending margin-page or toggle-zoom animation belongs to the outgoing
    // photo; drop it so it can't fire against the new one (or after unmount).
    clearPageTimer();
    cancelSettle();
    if (standalone) {
      sZoomed = false;
      sScale = 1;
      sCx = 0.5;
      sCy = 0.5;
    } else {
      view.resetZoom();
    }
    return () => {
      clearPageTimer();
      cancelSettle();
    };
  });

  // Keyboard zoom toggle (Z / Space) routes through the view store, which can't
  // know the per-photo `fit`; it bumps a nonce and we run the same centered
  // toggle the double-tap uses. Loupe only — compare panes keep local zoom.
  let lastKbNonce = 0;
  $effect(() => {
    const n = view.zoomToggleNonce;
    if (standalone || n === lastKbNonce) return;
    lastKbNonce = n;
    untrack(() => toggleZoom());
  });

  // Fit view uses the 2560px preview; zoomed view swaps in the full-res source
  // (unresized embedded JPEG for RAW, original file for images).
  const fitSrc = $derived(previewUrl(item));
  const fullSrc = $derived(cullantUrl(`full/${item.id}?v=${item.mtime}`));

  // Double-buffer the fit view: keep showing the previous photo until the new
  // preview has loaded, so rapid arrowing never flashes a blank pane.
  let displayedSrc = $state(untrack(() => previewUrl(item)));
  let displayedAlt = $state(untrack(() => item.name));
  /** True while the fit view shows the upscaled grid thumb as a stand-in. */
  let softPreview = $state(false);

  $effect(() => {
    const target = fitSrc;
    const alt = item.name;
    const progressive = settings.progressiveLoupe;
    const thumb = thumbUrl(item);
    if (target === untrack(() => displayedSrc)) return;
    const loader = new Image();
    // On error swap anyway — a broken image beats silently showing the wrong photo.
    loader.onload = loader.onerror = () => {
      if (timer !== undefined) clearTimeout(timer);
      softPreview = false;
      displayedSrc = target;
      displayedAlt = alt;
    };
    loader.src = target;
    // Progressive fit: if the sharp preview takes longer than a beat, paint
    // the (virtually always cached) grid thumb immediately, softened, and let
    // the preview replace it on load. Cached previews land before the timer,
    // so revisits never flash the soft frame.
    let timer: ReturnType<typeof setTimeout> | undefined;
    if (progressive) {
      timer = setTimeout(() => {
        softPreview = true;
        displayedSrc = thumb;
        displayedAlt = alt;
      }, 80);
    }
    return () => {
      // A newer target superseded this load; drop it so swaps stay in order.
      loader.onload = loader.onerror = null;
      if (timer !== undefined) clearTimeout(timer);
    };
  });

  // Authoritative full-resolution dimensions of the image AS DISPLAYED. The DB
  // stores raw EXIF sensor dims with orientation in a separate column, so for
  // rotated (portrait) shots item.width/height are swapped vs. what the
  // browser renders — trusting them blindly is what used to squash photos.
  // Prefer the loaded full image's natural size; before it loads, use DB dims
  // but swap them when the preview (already rendered, hence orientation-
  // correct) disagrees about portrait vs. landscape.
  const refDims = $derived.by(() => {
    if (fullNaturalW && fullNaturalH) return { w: fullNaturalW, h: fullNaturalH };
    let w = item.width ?? 0;
    let h = item.height ?? 0;
    if (w && h && previewNaturalW && previewNaturalH) {
      if (previewNaturalH > previewNaturalW !== h > w) [w, h] = [h, w];
    }
    if (!w || !h) {
      w = previewNaturalW;
      h = previewNaturalH;
    }
    return { w, h };
  });
  const refW = $derived(refDims.w);
  const refH = $derived(refDims.h);

  // The zoom model: ONE uniform scale factor in [fit, MAX_SCALE], where 1.0 is
  // one source pixel per CSS pixel and `fit` is the contain-scale at which the
  // whole photo is visible (capped at 1 — fit never upscales small photos).
  const fit = $derived.by(() => {
    if (!refW || !refH || !frameW || !frameH) return 1;
    return Math.min(1, frameW / refW, frameH / refH);
  });

  // Fit-anchored zoom: magnification is measured RELATIVE to fit (1× = the whole
  // image = "Fit"). Max magnification reaches 1:1 actual pixels for large photos
  // (for focus checks), or a 2× detail peek for photos already at/under fit —
  // never an arbitrary huge factor.
  const maxMag = $derived(Math.max(2, 1 / (fit || 1)));
  const maxScale = $derived(Math.min(MAX_SCALE, fit * maxMag));

  const clampToRange = (s: number) => Math.min(maxScale, Math.max(fit, s));

  const dispW = $derived(refW * z.scale);
  const dispH = $derived(refH * z.scale);

  const offset = $derived.by(() => {
    if (!z.zoomed || !dispW || !dispH) return { x: 0, y: 0 };
    // Center (cx, cy) of the image should land at the frame's center; clamp so
    // we never pan past edges, centering any axis smaller than the frame.
    return {
      x: clampOffset(frameW / 2 - z.cx * dispW, frameW, dispW),
      y: clampOffset(frameH / 2 - z.cy * dispH, frameH, dispH),
    };
  });

  function clampOffset(v: number, frameSize: number, dispSize: number): number {
    if (dispSize <= frameSize) return (frameSize - dispSize) / 2;
    return Math.max(frameSize - dispSize, Math.min(0, v));
  }

  /** Clamp a relative center so panning stops exactly at the image edges. */
  function clampCenter(c: number, frameSize: number, dispSize: number): number {
    if (dispSize <= frameSize) return 0.5;
    const half = frameSize / (2 * dispSize);
    return Math.min(1 - half, Math.max(half, c));
  }

  function onFitLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    previewNaturalW = img.naturalWidth;
    previewNaturalH = img.naturalHeight;
  }

  function onFullLoad(e: Event) {
    const img = e.currentTarget as HTMLImageElement;
    fullNaturalW = img.naturalWidth;
    fullNaturalH = img.naturalHeight;
  }

  function framePoint(e: { clientX: number; clientY: number }): { x: number; y: number } {
    if (!frame) return { x: frameW / 2, y: frameH / 2 };
    const r = frame.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  }

  /** Image-relative point (0..1) currently under the given frame coords. */
  function anchorUnder(px: number, py: number): { x: number; y: number } {
    if (z.zoomed && dispW && dispH) {
      return { x: clamp01((px - offset.x) / dispW), y: clamp01((py - offset.y) / dispH) };
    }
    // Fit view: the image sits letterboxed at fit scale, centered in the frame.
    const w = refW * fit;
    const h = refH * fit;
    return {
      x: clamp01((px - (frameW - w) / 2) / (w || 1)),
      y: clamp01((py - (frameH - h) / 2) / (h || 1)),
    };
  }

  /** Position the view so image point `u` sits under frame point (px, py) at scale s. */
  function centerOn(u: { x: number; y: number }, px: number, py: number, s: number) {
    const w = refW * s;
    const h = refH * s;
    z.cx = clampCenter(u.x + (frameW / 2 - px) / (w || 1), frameW, w);
    z.cy = clampCenter(u.y + (frameH / 2 - py) / (h || 1), frameH, h);
  }

  /** Focal-point zoom: whatever is under (px, py) stays under (px, py). */
  function zoomAt(px: number, py: number, s: number) {
    const u = anchorUnder(px, py);
    z.scale = s;
    z.zoomed = true;
    centerOn(u, px, py, s);
  }

  /**
   * Double-tap / double-click / Z-key zoom toggle — identical for mouse and
   * touch. The decision keys off the ACTUAL scale, not the `zoomed` mode flag
   * (which can be true at fit during a pinch entry/settle), so it always matches
   * what's on screen:
   *   - zoomed in at all (scale > fit)  -> animate back to the whole-image fit;
   *   - at fit                          -> DOUBLE_TAP_MAG × fit, centered
   *                                        (Twitter-style), capped at 1:1 pixels.
   * Both directions animate briefly. A photo whose target is still not above
   * fit stays put (the least-surprising no-op).
   */
  function toggleZoom() {
    // Swallow a duplicate toggle (e.g. our manual touch double-tap plus a
    // browser-synthesized dblclick) so the two don't cancel each other out.
    const now = performance.now();
    if (now - lastToggleTime < TOGGLE_GUARD_MS) return;
    lastToggleTime = now;

    if (!refW || !refH) return;
    cancelSettle();

    if (z.zoomed && z.scale > fit + ZOOM_EPS) {
      // Currently zoomed in -> animate back to the fit view (settleToFit keeps
      // the full image mounted through the transition, then drops zoom mode).
      settleToFit();
      return;
    }
    // At fit -> DOUBLE_TAP_MAG × fit, centered. clampToRange caps it at 1:1 for
    // large photos, or at the modest detail zoom for small ones.
    const target = clampToRange(fit * DOUBLE_TAP_MAG);
    if (target <= fit + ZOOM_EPS) return; // nothing beyond fit to reveal
    animateZoomIn(target);
  }

  // Centered (Twitter-style) zoom-in with a brief transition. The full image is
  // first mounted at the fit scale (matching what's on screen), then bumped to
  // `target` on a later frame so the `.settling` CSS transition animates from
  // fit -> target instead of snapping. cx = cy = 0.5 keeps equal image on the
  // left and right, regardless of where the tap/click landed.
  function animateZoomIn(target: number) {
    z.zoomed = true;
    z.scale = fit;
    z.cx = 0.5;
    z.cy = 0.5;
    settling = true;
    if (toggleRaf) cancelAnimationFrame(toggleRaf);
    toggleRaf = requestAnimationFrame(() => {
      toggleRaf = requestAnimationFrame(() => {
        toggleRaf = 0;
        z.scale = target;
        z.cx = 0.5;
        z.cy = 0.5;
        settleTimer = setTimeout(() => {
          settleTimer = undefined;
          settling = false;
        }, 180);
      });
    });
  }

  function onDblClick() {
    toggleZoom();
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    if (e.ctrlKey) {
      if (!refW || !refH || !frameW || !frameH) return;
      const factor = e.deltaY < 0 ? WHEEL_ZOOM : 1 / WHEEL_ZOOM;
      const current = z.zoomed ? z.scale : fit;
      const next = clampToRange(current * factor);
      if (!z.zoomed && next <= fit) return; // already fully zoomed out
      if (z.zoomed && next <= fit + 1e-6) {
        // Wheeling out lands on fit: return to the clean fit view.
        z.zoomed = false;
        return;
      }
      const p = framePoint(e);
      zoomAt(p.x, p.y, next);
      return;
    }
    // Plain wheel navigates photos: accumulate deltas so one physical notch
    // (or an equivalent trackpad swipe) steps exactly one photo. Horizontal
    // scroll (trackpad swipe, or Shift+wheel which browsers report as
    // deltaX) navigates the same way as vertical.
    const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (Math.sign(delta) !== Math.sign(wheelAccum)) wheelAccum = 0;
    wheelAccum += delta;
    if (Math.abs(wheelAccum) >= WHEEL_NAV_THRESHOLD) {
      session.moveFocus(wheelAccum > 0 ? 1 : -1);
      wheelAccum = 0;
    }
  }

  function panBy(dx: number, dy: number) {
    if (!dispW || !dispH) return;
    z.cx = clampCenter(z.cx - dx / dispW, frameW, dispW);
    z.cy = clampCenter(z.cy - dy / dispH, frameH, dispH);
  }

  // Rubber-band release / toggle zoom: briefly animate width+transform, then
  // drop out of zoom mode when back at fit (the fit view renders identically,
  // so the swap is invisible).
  let settling = $state(false);
  let settleTimer: ReturnType<typeof setTimeout> | undefined;
  // A discrete toggle zoom-IN mounts the full image at fit, then jumps to the
  // target on a later frame so the CSS transition has a "from" state; this
  // holds that pending frame request.
  let toggleRaf = 0;
  // Deferred e-reader page: a margin tap waits out the double-tap window before
  // paging, so a double-tap in the margin zooms instead of paging twice.
  let pageTimer: ReturnType<typeof setTimeout> | undefined;

  function clearPageTimer() {
    if (pageTimer !== undefined) {
      clearTimeout(pageTimer);
      pageTimer = undefined;
    }
  }

  function cancelSettle() {
    if (settleTimer !== undefined) {
      clearTimeout(settleTimer);
      settleTimer = undefined;
    }
    if (toggleRaf) {
      cancelAnimationFrame(toggleRaf);
      toggleRaf = 0;
    }
    settling = false;
  }

  function settleToFit() {
    settling = true;
    z.scale = fit;
    z.cx = 0.5;
    z.cy = 0.5;
    settleTimer = setTimeout(() => {
      settleTimer = undefined;
      settling = false;
      if (z.zoomed && z.scale <= fit + 1e-6) z.zoomed = false;
    }, 180);
  }

  function onPointerDown(e: PointerEvent) {
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);
    cancelSettle();

    if (e.pointerType === "touch") {
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pointers.size === 2) {
        // Second finger down: begin a pinch anchored on the finger midpoint.
        pinchedThisGesture = true;
        const [a, b] = [...pointers.values()];
        pinchStartDist = Math.hypot(a.x - b.x, a.y - b.y);
        pinchStartScale = z.zoomed ? z.scale : fit;
        const m = framePoint({ clientX: (a.x + b.x) / 2, clientY: (a.y + b.y) / 2 });
        pinchAnchor = anchorUnder(m.x, m.y);
        if (!z.zoomed) {
          // Enter zoom seamlessly at the fit scale, so the pinch ramps up
          // continuously from exactly what was on screen.
          z.scale = fit;
          z.zoomed = true;
          centerOn(pinchAnchor, m.x, m.y, fit);
        }
        dragging = false;
      } else if (pointers.size === 1) {
        pinchedThisGesture = false;
        swipeStartX = e.clientX;
        swipeStartY = e.clientY;
        lastX = e.clientX;
        lastY = e.clientY;
        dragging = z.zoomed; // one finger pans only when already zoomed
      }
      return;
    }

    // Mouse / pen: drag to pan when zoomed (unchanged desktop behavior).
    if (!z.zoomed) return;
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
  }

  function onPointerMove(e: PointerEvent) {
    if (e.pointerType === "touch") {
      if (!pointers.has(e.pointerId)) return;
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });

      if (pointers.size >= 2) {
        if (pinchStartDist <= 0) return;
        const [a, b] = [...pointers.values()];
        const d = Math.hypot(a.x - b.x, a.y - b.y);
        const raw = pinchStartScale * (d / pinchStartDist);
        // Below fit the zoom resists (rubber band) instead of deforming or
        // sticking at an arbitrary floor; release snaps it back to fit.
        const s = Math.min(maxScale, raw < fit ? fit * Math.pow(raw / fit, RUBBER) : raw);
        // Keep the image point grabbed at pinch start under the CURRENT finger
        // midpoint: focal-point zoom and two-finger pan in one motion.
        const m = framePoint({ clientX: (a.x + b.x) / 2, clientY: (a.y + b.y) / 2 });
        z.scale = s;
        centerOn(pinchAnchor, m.x, m.y, s);
        return;
      }
      // One finger, zoomed: pan. Not zoomed: nothing live (tap/swipe on up).
      if (z.zoomed && dragging) {
        panBy(e.clientX - lastX, e.clientY - lastY);
        lastX = e.clientX;
        lastY = e.clientY;
      }
      return;
    }

    if (!dragging || !z.zoomed || !dispW || !dispH) return;
    panBy(e.clientX - lastX, e.clientY - lastY);
    lastX = e.clientX;
    lastY = e.clientY;
  }

  function onPointerUp(e: PointerEvent) {
    if (e.pointerType === "touch") {
      const wasSingle = pointers.size === 1;
      const wasPinching = pinchStartDist > 0;
      pointers.delete(e.pointerId);
      if (pointers.size < 2) pinchStartDist = 0;

      // Pinch ended below (or at) fit: spring back and leave zoom mode.
      if (wasPinching && pointers.size < 2 && z.zoomed && z.scale <= fit + 1e-6) {
        settleToFit();
      }

      if (wasSingle && !pinchedThisGesture) {
        const dx = e.clientX - swipeStartX;
        const dy = e.clientY - swipeStartY;
        if (Math.hypot(dx, dy) <= TAP_SLOP) {
          const p = framePoint(e);
          // Double-tap detection runs FIRST, for margin and center taps alike.
          // Match the two taps on their finger-DOWN points (swipeStartX/Y):
          // stable even if a tap incidentally nudged the pan while zoomed, so
          // the zoom-OUT double-tap is as reliable as the zoom-IN one.
          const now = performance.now();
          const nearLast = Math.hypot(swipeStartX - lastTapX, swipeStartY - lastTapY) < DOUBLE_TAP_DIST;
          if (now - lastTapTime <= DOUBLE_TAP_MS && nearLast) {
            // A quick second tap in place toggles fit <-> zoom, and cancels any
            // margin page still waiting out the double-tap window — so a
            // double-tap in the margin zooms once and never pages.
            lastTapTime = 0;
            clearPageTimer();
            toggleZoom();
          } else {
            // First tap: remember it. If it landed in the left/right margin of
            // the fit view, schedule an e-reader page to the prev/next photo —
            // but DEFER it by the double-tap window so a second tap can cancel
            // it and zoom instead. A center tap just waits for a possible
            // double-tap; a lone margin tap pages once the window lapses.
            lastTapTime = now;
            lastTapX = swipeStartX;
            lastTapY = swipeStartY;
            const edge = frameW * EDGE_TAP_FRAC;
            if (!z.zoomed && frameW && (p.x < edge || p.x > frameW - edge)) {
              const dir = p.x < edge ? -1 : 1;
              clearPageTimer();
              pageTimer = setTimeout(() => {
                pageTimer = undefined;
                lastTapTime = 0;
                session.moveFocus(dir);
              }, DOUBLE_TAP_MS);
            }
          }
        } else if (!z.zoomed && Math.abs(dx) > SWIPE_NAV_THRESHOLD && Math.abs(dx) > Math.abs(dy) * 1.3) {
          // A single-finger horizontal flick in fit view navigates photos.
          session.moveFocus(dx < 0 ? 1 : -1);
        }
      }

      // Lifting one finger of a pinch: let the remaining finger keep panning.
      if (pointers.size === 1 && z.zoomed) {
        const [p] = [...pointers.values()];
        lastX = p.x;
        lastY = p.y;
        dragging = true;
      } else if (pointers.size === 0) {
        dragging = false;
      }
      return;
    }

    dragging = false;
  }

  function onPointerCancel(e: PointerEvent) {
    pointers.delete(e.pointerId);
    if (pointers.size < 2) pinchStartDist = 0;
    if (pointers.size === 0) dragging = false;
  }
</script>

<div
  class="frame"
  class:zoomed={z.zoomed}
  bind:this={frame}
  bind:clientWidth={frameW}
  bind:clientHeight={frameH}
  ondblclick={onDblClick}
  onwheel={onWheel}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerCancel}
  role="img"
>
  {#if z.zoomed}
    <!-- Only the WIDTH is set: height follows the image's intrinsic ratio, so
         the single uniform scale factor can never deform the photo, even when
         stored metadata disagrees with the rendered orientation. -->
    <img
      src={fullSrc}
      alt={item.name}
      style="transform: translate({offset.x}px, {offset.y}px); width: {dispW}px;"
      class="full"
      class:settling
      draggable="false"
      onload={onFullLoad}
    />
  {:else}
    <img
      src={displayedSrc}
      alt={displayedAlt}
      class="fit"
      class:soft={softPreview}
      draggable="false"
      onload={onFitLoad}
    />
  {/if}
</div>

<style>
  .frame {
    position: relative;
    flex: 1;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-stage);
    /* We handle swipe/pinch/pan ourselves via pointer events. */
    touch-action: none;
  }

  .frame.zoomed {
    cursor: grab;
    display: block;
  }

  .frame.zoomed:active {
    cursor: grabbing;
  }

  img.fit {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    user-select: none;
  }

  /* Upscaled grid thumb standing in while the sharp preview decodes. */
  img.fit.soft {
    filter: blur(4px);
  }

  img.full {
    position: absolute;
    top: 0;
    left: 0;
    max-width: none;
    max-height: none;
    height: auto; /* aspect ratio always follows the source pixels */
    user-select: none;
  }

  /* Brief transition for the discrete toggle zoom (double-tap / Z) and the
     rubber-band spring back to fit after an over-pinch release. Live pinch,
     pan and drag never carry this class, so they stay 1:1 with the finger. */
  img.full.settling {
    transition:
      width 160ms ease-out,
      transform 160ms ease-out;
  }
</style>
