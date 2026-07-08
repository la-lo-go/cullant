<script lang="ts">
  import { previewUrl, cullantUrl, type ItemLite } from "../api";
  import { view } from "../stores/view.svelte";

  let { item }: { item: ItemLite } = $props();

  let frame = $state<HTMLDivElement | null>(null);
  let frameW = $state(0);
  let frameH = $state(0);
  let naturalW = $state(0);
  let naturalH = $state(0);
  let dragging = false;
  let lastX = 0;
  let lastY = 0;

  // Fit view uses the 2560px preview; 100% view swaps in the full-res source
  // (unresized embedded JPEG for RAW, original file for images).
  const src = $derived(
    view.zoomed
      ? cullantUrl(`full/${item.id}?v=${item.mtime}`)
      : previewUrl(item)
  );

  // At 100%, one image pixel = one CSS pixel of the ORIGINAL resolution.
  const fullW = $derived(item.width ?? naturalW);
  const fullH = $derived(item.height ?? naturalH);

  const offset = $derived.by(() => {
    if (!view.zoomed || !fullW || !fullH) return { x: 0, y: 0 };
    // Center (cx, cy) of the image should land at the frame's center.
    const x = frameW / 2 - view.cx * fullW;
    const y = frameH / 2 - view.cy * fullH;
    // Clamp so we never pan past edges (unless image smaller than frame).
    const minX = Math.min(0, frameW - fullW);
    const minY = Math.min(0, frameH - fullH);
    return {
      x: Math.max(minX, Math.min(0, x)) || Math.min(0, x),
      y: Math.max(minY, Math.min(0, y)) || Math.min(0, y),
    };
  });

  function onImageLoad(e: Event) {
    const img = e.target as HTMLImageElement;
    naturalW = img.naturalWidth;
    naturalH = img.naturalHeight;
  }

  function toRelative(e: MouseEvent): { x: number; y: number } {
    if (!frame) return { x: 0.5, y: 0.5 };
    const rect = frame.getBoundingClientRect();
    if (view.zoomed) {
      return { x: view.cx, y: view.cy };
    }
    // In fit mode the image is letterboxed; approximate via frame coords.
    return {
      x: (e.clientX - rect.left) / rect.width,
      y: (e.clientY - rect.top) / rect.height,
    };
  }

  function onDblClick(e: MouseEvent) {
    const p = toRelative(e);
    view.toggleZoom(p.x, p.y);
  }

  function onPointerDown(e: PointerEvent) {
    if (!view.zoomed) return;
    dragging = true;
    lastX = e.clientX;
    lastY = e.clientY;
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging || !view.zoomed || !fullW || !fullH) return;
    const dx = e.clientX - lastX;
    const dy = e.clientY - lastY;
    lastX = e.clientX;
    lastY = e.clientY;
    view.cx = Math.min(1, Math.max(0, view.cx - dx / fullW));
    view.cy = Math.min(1, Math.max(0, view.cy - dy / fullH));
  }

  function onPointerUp() {
    dragging = false;
  }
</script>

<div
  class="frame"
  class:zoomed={view.zoomed}
  bind:this={frame}
  bind:clientWidth={frameW}
  bind:clientHeight={frameH}
  ondblclick={onDblClick}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  role="img"
>
  {#if view.zoomed}
    <img
      {src}
      alt={item.name}
      style="transform: translate({offset.x}px, {offset.y}px); width: {fullW}px; height: {fullH}px;"
      class="full"
      draggable="false"
      onload={onImageLoad}
    />
  {:else}
    <img {src} alt={item.name} class="fit" draggable="false" onload={onImageLoad} />
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
</style>
