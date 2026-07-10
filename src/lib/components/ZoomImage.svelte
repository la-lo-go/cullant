<script lang="ts">
  import { untrack } from "svelte";
  import { previewUrl, thumbUrl, cullantUrl, type ItemLite } from "../api";
  import { view, clampScale } from "../stores/view.svelte";
  import { session } from "../stores/session.svelte";
  import { settings } from "../stores/settings.svelte";

  let { item, standalone = false }: { item: ItemLite; standalone?: boolean } = $props();

  let frame = $state<HTMLDivElement | null>(null);
  let frameW = $state(0);
  let frameH = $state(0);
  let naturalW = $state(0);
  let naturalH = $state(0);
  let dragging = false;
  let lastX = 0;
  let lastY = 0;
  let wheelAccum = 0;

  // Active touch points (by pointerId), plus pinch/swipe gesture state. Only
  // touch pointers use these; mouse/pen keep the classic drag-to-pan behavior.
  const pointers = new Map<number, { x: number; y: number }>();
  let pinchStartDist = 0;
  let pinchStartScale = 1;
  let swipeStartX = 0;
  let swipeStartY = 0;

  const WHEEL_ZOOM = 1.15;
  const WHEEL_NAV_THRESHOLD = 50;
  const SWIPE_NAV_THRESHOLD = 55;

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

  // Standalone zoom is non-persistent: reset to fit whenever the item changes.
  $effect(() => {
    void item.id;
    if (!standalone) return;
    sZoomed = false;
    sScale = 1;
    sCx = 0.5;
    sCy = 0.5;
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

  // At scale 1.0, one image pixel = one CSS pixel of the ORIGINAL resolution.
  const fullW = $derived(item.width ?? naturalW);
  const fullH = $derived(item.height ?? naturalH);
  const dispW = $derived(fullW * z.scale);
  const dispH = $derived(fullH * z.scale);

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

  function onImageLoad(e: Event) {
    const img = e.target as HTMLImageElement;
    naturalW = img.naturalWidth;
    naturalH = img.naturalHeight;
  }

  function toRelative(e: MouseEvent): { x: number; y: number } {
    if (!frame) return { x: 0.5, y: 0.5 };
    const rect = frame.getBoundingClientRect();
    if (z.zoomed) {
      return { x: z.cx, y: z.cy };
    }
    // In fit mode the image is letterboxed; approximate via frame coords.
    return {
      x: (e.clientX - rect.left) / rect.width,
      y: (e.clientY - rect.top) / rect.height,
    };
  }

  function toggleZoom(atX: number, atY: number) {
    if (!z.zoomed) {
      z.cx = atX;
      z.cy = atY;
      z.scale = 1;
    }
    z.zoomed = !z.zoomed;
  }

  function onDblClick(e: MouseEvent) {
    const p = toRelative(e);
    toggleZoom(p.x, p.y);
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    if (e.ctrlKey) {
      const factor = e.deltaY < 0 ? WHEEL_ZOOM : 1 / WHEEL_ZOOM;
      if (z.zoomed) {
        z.scale = clampScale(z.scale * factor);
      } else if (factor > 1 && fullW && fullH && frameW && frameH) {
        // Ctrl+wheel-in from fit: enter zoom centered on the cursor, starting
        // from the fit scale so the zoom ramps up continuously.
        const p = toRelative(e);
        const fit = Math.min(1, frameW / fullW, frameH / fullH);
        z.cx = p.x;
        z.cy = p.y;
        z.scale = clampScale(fit * factor);
        z.zoomed = true;
      }
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

  function onSliderInput(e: Event) {
    const input = e.currentTarget as HTMLInputElement;
    z.scale = clampScale(Number(input.value) / 100);
  }

  // The slider must never keep keyboard focus — arrows navigate photos.
  function blurSlider(e: Event) {
    (e.currentTarget as HTMLElement).blur();
  }

  function frameRelative(clientX: number, clientY: number): { x: number; y: number } {
    if (!frame) return { x: 0.5, y: 0.5 };
    const r = frame.getBoundingClientRect();
    return { x: (clientX - r.left) / r.width, y: (clientY - r.top) / r.height };
  }

  function fitScale(): number {
    return Math.min(1, frameW / (fullW || 1), frameH / (fullH || 1));
  }

  function panBy(dx: number, dy: number) {
    if (!dispW || !dispH) return;
    z.cx = clamp01(z.cx - dx / dispW);
    z.cy = clamp01(z.cy - dy / dispH);
  }

  function onPointerDown(e: PointerEvent) {
    (e.target as HTMLElement).setPointerCapture?.(e.pointerId);

    if (e.pointerType === "touch") {
      pointers.set(e.pointerId, { x: e.clientX, y: e.clientY });
      if (pointers.size === 2) {
        // Second finger down: begin a pinch, entering zoom from fit if needed.
        const [a, b] = [...pointers.values()];
        pinchStartDist = Math.hypot(a.x - b.x, a.y - b.y);
        pinchStartScale = z.zoomed ? z.scale : fitScale();
        if (!z.zoomed) {
          const p = frameRelative((a.x + b.x) / 2, (a.y + b.y) / 2);
          z.cx = p.x;
          z.cy = p.y;
          z.scale = pinchStartScale;
          z.zoomed = true;
        }
        dragging = false;
      } else if (pointers.size === 1) {
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
        const [a, b] = [...pointers.values()];
        const d = Math.hypot(a.x - b.x, a.y - b.y);
        if (pinchStartDist > 0) z.scale = clampScale(pinchStartScale * (d / pinchStartDist));
        return;
      }
      // One finger, zoomed: pan. Not zoomed: nothing live (swipe decided on up).
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
      pointers.delete(e.pointerId);
      if (pointers.size < 2) pinchStartDist = 0;

      // A single-finger horizontal flick in fit view navigates photos.
      if (wasSingle && !z.zoomed) {
        const dx = e.clientX - swipeStartX;
        const dy = e.clientY - swipeStartY;
        if (Math.abs(dx) > SWIPE_NAV_THRESHOLD && Math.abs(dx) > Math.abs(dy) * 1.3) {
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
    <img
      src={fullSrc}
      alt={item.name}
      style="transform: translate({offset.x}px, {offset.y}px); width: {dispW}px; height: {dispH}px;"
      class="full"
      draggable="false"
      onload={onImageLoad}
    />
    <div class="zoom-ctl">
      <input
        type="range"
        min="10"
        max="400"
        step="1"
        value={Math.round(z.scale * 100)}
        aria-label="Zoom level"
        oninput={onSliderInput}
        onchange={blurSlider}
        onpointerup={blurSlider}
        onpointerdown={(e) => e.stopPropagation()}
        ondblclick={(e) => e.stopPropagation()}
      />
      <span class="pct">{Math.round(z.scale * 100)}%</span>
    </div>
  {:else}
    <img
      src={displayedSrc}
      alt={displayedAlt}
      class="fit"
      class:soft={softPreview}
      draggable="false"
      onload={onImageLoad}
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
    user-select: none;
  }

  .zoom-ctl {
    position: absolute;
    right: 10px;
    bottom: 34px;
    z-index: 4;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.45);
    cursor: default;
  }

  .zoom-ctl input[type="range"] {
    width: 90px;
    height: 12px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .zoom-ctl .pct {
    min-width: 36px;
    text-align: right;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.75);
    user-select: none;
  }
</style>
