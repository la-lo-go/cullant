<script lang="ts">
  import { untrack } from "svelte";
  import { previewUrl, cullantUrl, type ItemLite } from "../api";
  import { view, clampScale } from "../stores/view.svelte";
  import { session } from "../stores/session.svelte";

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

  const WHEEL_ZOOM = 1.15;
  const WHEEL_NAV_THRESHOLD = 50;

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

  $effect(() => {
    const target = fitSrc;
    const alt = item.name;
    if (target === displayedSrc) return;
    const loader = new Image();
    // On error swap anyway — a broken image beats silently showing the wrong photo.
    loader.onload = loader.onerror = () => {
      displayedSrc = target;
      displayedAlt = alt;
    };
    loader.src = target;
    return () => {
      // A newer target superseded this load; drop it so swaps stay in order.
      loader.onload = loader.onerror = null;
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
    // (or an equivalent trackpad swipe) steps exactly one photo.
    if (Math.sign(e.deltaY) !== Math.sign(wheelAccum)) wheelAccum = 0;
    wheelAccum += e.deltaY;
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

  function onPointerDown(e: PointerEvent) {
    if (!z.zoomed) return;
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging || !z.zoomed || !dispW || !dispH) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    lastX = e.clientX;
    lastY = e.clientY;
    z.cx = Math.min(1, Math.max(0, z.cx - dx / dispW));
    z.cy = Math.min(1, Math.max(0, z.cy - dy / dispH));
  }

  function onPointerUp() {
    dragging = false;
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
    <img src={displayedSrc} alt={displayedAlt} class="fit" draggable="false" onload={onImageLoad} />
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
    background: #131316;
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
    accent-color: #8fa6ff;
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
