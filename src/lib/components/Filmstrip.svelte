<script lang="ts">
  import { thumbUrl, videoUrl, type ItemLite } from "../api";
  import { session } from "../stores/session.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
  import Scissors from "@lucide/svelte/icons/scissors";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";

  let { items }: { items: ItemLite[] } = $props();

  const CELL = 96;
  const OVERSCAN = 6;

  let strip = $state<HTMLDivElement | null>(null);
  let scrollLeft = $state(0);
  let width = $state(0);

  const first = $derived(Math.max(0, Math.floor(scrollLeft / CELL) - OVERSCAN));
  const last = $derived(
    Math.min(items.length, Math.ceil((scrollLeft + width) / CELL) + OVERSCAN)
  );

  // Keyed by file id in the template so each frame keeps a stable <img>:
  // scrolling adds/removes cells instead of reassigning `src` on reused nodes,
  // which previously left a node showing its old thumbnail until the new one
  // decoded — a rapid flicker on fast (right-to-left) scroll.
  const visible = $derived.by(() => {
    const out: { item: ItemLite; index: number; x: number }[] = [];
    for (let i = first; i < last; i++) {
      out.push({ item: items[i], index: i, x: i * CELL });
    }
    return out;
  });

  // Keep the focused frame centered as the user arrows through the shoot.
  // A single step scrolls smoothly; while stepping fast (holding an arrow key)
  // we fall back to instant so recentring never lags behind the selection.
  const RAPID_STEP_MS = 180;
  let lastRecentre = 0;
  $effect(() => {
    if (!strip) return;
    const target = Math.max(0, session.focusedIndex * CELL - width / 2 + CELL / 2);
    const now = performance.now();
    const rapid = now - lastRecentre < RAPID_STEP_MS;
    lastRecentre = now;
    strip.scrollTo({ left: target, behavior: rapid ? "auto" : "smooth" });
  });

  // Drag the top edge to resize; dragging it below COLLAPSE_AT hides the strip
  // (the peek arrow at the bottom, and the F toggle, bring it back).
  const MIN_H = 72;
  const MAX_H = 320;
  const COLLAPSE_AT = 56;

  let resizing = $state(false);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onResizeMove(e: PointerEvent) {
    if (!resizing || !strip) return;
    const h = strip.getBoundingClientRect().bottom - e.clientY;
    if (h < COLLAPSE_AT) {
      resizing = false;
      session.setShowFilmstrip(false);
      return;
    }
    session.filmstripHeight = Math.min(MAX_H, Math.max(MIN_H, h));
  }

  function endResize(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    session.setFilmstripHeight(session.filmstripHeight);
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      // capture may already be gone
    }
  }

  // Tap-vs-scroll detection for cells: a finger dragging the strip should scroll
  // it, not steal focus to whatever cell it first landed on. We record the
  // pointer down position/index and only select on pointerup if it barely moved.
  const TAP_SLOP = 8; // px of movement still counted as a tap (not a scroll)
  let tapPointerId: number | null = null;
  let tapStartX = 0;
  let tapStartY = 0;
  let tapIndex = -1;

  function onCellPointerDown(e: PointerEvent, index: number) {
    // Do NOT setPointerCapture here — that would swallow the strip's native
    // horizontal scroll. We only read coordinates.
    tapPointerId = e.pointerId;
    tapStartX = e.clientX;
    tapStartY = e.clientY;
    tapIndex = index;
  }

  function onCellPointerUp(e: PointerEvent) {
    if (tapPointerId !== e.pointerId || tapIndex < 0) return;
    const moved =
      Math.abs(e.clientX - tapStartX) > TAP_SLOP ||
      Math.abs(e.clientY - tapStartY) > TAP_SLOP;
    if (!moved) {
      session.focusedIndex = tapIndex;
      session.selectionAnchor = tapIndex;
    }
    resetTap();
  }

  function resetTap() {
    tapPointerId = null;
    tapIndex = -1;
  }
</script>

{#if session.showFilmstrip}
  <div class="filmstrip" style="height: {session.filmstripHeight}px">
    <!-- Top-edge resize handle. -->
    <div
      class="resize-handle"
      class:resizing
      role="separator"
      aria-orientation="horizontal"
      aria-label="Resize filmstrip"
      onpointerdown={startResize}
      onpointermove={onResizeMove}
      onpointerup={endResize}
      onpointercancel={endResize}
    ></div>
    <div
      class="strip"
      id="filmstrip-scroll"
      bind:this={strip}
      bind:clientWidth={width}
      onscroll={() => strip && (scrollLeft = strip.scrollLeft)}
    >
      <div class="canvas" style="width:{items.length * CELL}px">
        {#each visible as v (v.item.id)}
          <div
            class="cell"
            class:focused={v.index === session.focusedIndex}
            style="transform: translateX({v.x}px); width:{CELL}px"
            onpointerdown={(e) => onCellPointerDown(e, v.index)}
            onpointerup={onCellPointerUp}
            onpointercancel={resetTap}
            role="button"
            tabindex="-1"
          >
            {#if v.item.kind === 2}
              <video src={videoUrl(v.item)} preload="metadata" muted></video>
            {:else}
              <img src={thumbUrl(v.item)} alt="" decoding="async" draggable="false" />
            {/if}
            {#if session.mirrorMode && v.item.groupSize > 1}
              <span class="chip" class:split={v.item.decoupled}>
                {#if v.item.decoupled}<Scissors size={8} /><span>SPLIT</span>{:else}RAW+JPG{/if}
              </span>
            {/if}
            {#if v.item.flag !== 0}
              <span class="dot" class:pick={v.item.flag === 1} class:reject={v.item.flag === -1}></span>
            {/if}
          </div>
        {/each}
      </div>
    </div>
    <!-- Overlay scrollbar floating over the bottom edge of the cells, aligned
         with the strip's safe-area box. -->
    <div class="strip-overlay">
      <OverlayScrollbar
        orientation="horizontal"
        viewport={width}
        content={items.length * CELL}
        position={scrollLeft}
        controls="filmstrip-scroll"
        onSeek={(pos) => strip?.scrollTo({ left: pos })}
      />
    </div>
  </div>
{:else}
  <button
    class="strip-peek"
    title="Show filmstrip (F)"
    aria-label="Show filmstrip"
    onclick={() => session.setShowFilmstrip(true)}
  >
    <ChevronUp size={16} />
  </button>
{/if}

<style>
  .filmstrip {
    position: relative;
    flex: none;
    background: var(--surface);
    border-top: 1px solid var(--border);
    /* Edge-to-edge: the surface bleeds under a landscape navigation bar or
       cutout while the cells stay inside the safe area. */
    padding-left: var(--safe-left);
    padding-right: var(--safe-right);
  }

  .strip {
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    /* Native bar hidden: a classic scrollbar would carve 8px out of the cell
       height. OverlayScrollbar floats over the thumbnails instead. */
    scrollbar-width: none;
  }

  .strip::-webkit-scrollbar {
    display: none;
  }

  /* Positioning context for the overlay scrollbar, matching the strip's
     safe-area-inset box so the thumb tracks the visible cells. */
  .strip-overlay {
    position: absolute;
    top: 0;
    bottom: 0;
    left: var(--safe-left);
    right: var(--safe-right);
    pointer-events: none;
  }

  /* Grabbable strip straddling the top edge; a hairline reveals on hover/drag. */
  .resize-handle {
    position: absolute;
    top: -3px;
    left: 0;
    right: 0;
    height: 7px;
    cursor: row-resize;
    z-index: 25;
    touch-action: none;
  }

  .resize-handle::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 3px;
    height: 2px;
    background: transparent;
    transition: background 0.12s;
  }

  .resize-handle:hover::after,
  .resize-handle.resizing::after {
    background: var(--accent);
  }

  /* Peek tab shown at the bottom edge when the filmstrip is collapsed. It floats
     (absolute) so the collapsed strip reserves NO height — the freed band goes
     back to the viewer/compare stage above. Anchored to the view's bottom edge
     (its parent .viewer/.compare is position:relative). */
  .strip-peek {
    position: absolute;
    left: 50%;
    bottom: var(--safe-bottom);
    transform: translateX(-50%);
    z-index: 10;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border);
    border-bottom: none;
    border-radius: 6px 6px 0 0;
    background: var(--surface);
    color: var(--accent);
    cursor: pointer;
  }

  .strip-peek:hover {
    background: var(--hover);
    border-color: var(--accent);
  }

  .canvas {
    position: relative;
    height: 100%;
  }

  .cell {
    position: absolute;
    top: 0;
    height: 100%;
    /* 8px (not 6px) vertical padding so the thumbnail bottom clears the overlay
       scrollbar: its pill sits 2-6px above the panel edge, so 6px left the
       image flush to the pill while the pill kept a 2px gap below it. 8px lifts
       the image 2px off the pill, mirroring that 2px gap above and below. */
    padding: 8px 3px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
  }

  .cell.focused {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  img,
  video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: 3px;
    user-select: none;
  }

  .chip {
    position: absolute;
    top: 8px;
    left: 5px;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 8px;
    font-weight: 600;
    padding: 1px 4px;
    border-radius: 3px;
    background: rgba(0, 0, 0, 0.55);
    color: #8fd0ff;
    pointer-events: none;
  }

  .chip.split {
    color: #ffb86b;
  }

  .dot {
    position: absolute;
    top: 8px;
    right: 6px;
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .dot.pick {
    background: #6be675;
  }

  .dot.reject {
    background: #ff6b6b;
  }
</style>
