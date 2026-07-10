<script lang="ts">
  import { thumbUrl, videoUrl, type ItemLite } from "../api";
  import { session } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import { view } from "../stores/view.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Play from "@lucide/svelte/icons/play";
  import FileWarning from "@lucide/svelte/icons/file-warning";
  import { edgeBounce } from "../anim";

  let { items }: { items: ItemLite[] } = $props();

  const OVERSCAN_ROWS = 2;
  const LONG_PRESS_MS = 400; // touch: hold this long to start a marquee
  const MARGIN_Y = 14; // breathing room above the first row and below the last

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  let viewport = $state<HTMLDivElement | null>(null);
  let canvasEl = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let width = $state(0);
  let height = $state(0);

  // Bounce the grid when arrow keys try to move past the first/last cell.
  // Track the last-seen bump so switching into the grid view doesn't replay a
  // stale bounce on mount.
  let lastBump = session.edgeBump.n;
  $effect(() => {
    const b = session.edgeBump;
    if (b.n === lastBump || !canvasEl) return;
    lastBump = b.n;
    edgeBounce(canvasEl, b.dir, b.axis);
  });

  // Cell pitch (thumbnail + label + gap). Smaller on narrow viewports so phones
  // show several columns instead of one huge cell.
  const CELL = $derived(width > 0 && width < 520 ? 120 : 200);

  const cols = $derived(Math.max(1, Math.floor(width / CELL)));
  // Center the block of columns: split the leftover horizontal space evenly so
  // equal margins sit on both edges instead of collecting all on the right.
  const padX = $derived(Math.max(0, (width - cols * CELL) / 2));
  const totalRows = $derived(Math.ceil(items.length / cols));
  // The grid is shifted down by MARGIN_Y, so offset the visible-window math by it.
  const firstRow = $derived(
    Math.max(0, Math.floor((scrollTop - MARGIN_Y) / CELL) - OVERSCAN_ROWS)
  );
  const lastRow = $derived(
    Math.min(totalRows, Math.ceil((scrollTop + height - MARGIN_Y) / CELL) + OVERSCAN_ROWS)
  );

  // Report layout so keyboard ↑/↓ move one visual row.
  $effect(() => {
    session.gridCols = cols;
  });

  // Keep the focused cell in view when keyboard navigation moves it.
  $effect(() => {
    const row = Math.floor(session.focusedIndex / cols);
    if (!viewport) return;
    const top = MARGIN_Y + row * CELL;
    const bottom = top + CELL;
    if (top < viewport.scrollTop) {
      viewport.scrollTo({ top });
    } else if (bottom > viewport.scrollTop + height) {
      viewport.scrollTo({ top: bottom - height });
    }
  });

  // Only the visible window is materialized. The #each below is deliberately
  // unkeyed: Svelte reuses DOM nodes positionally, which recycles <img>
  // elements as the user scrolls instead of churning the DOM.
  const visible = $derived.by(() => {
    const out: { item: ItemLite; index: number; x: number; y: number }[] = [];
    for (let row = firstRow; row < lastRow; row++) {
      for (let col = 0; col < cols; col++) {
        const index = row * cols + col;
        if (index >= items.length) break;
        out.push({ item: items[index], index, x: padX + col * CELL, y: MARGIN_Y + row * CELL });
      }
    }
    return out;
  });

  function onScroll() {
    if (viewport) scrollTop = viewport.scrollTop;
  }

  // --- pointer selection: click routing + drag marquee ---
  const DRAG_THRESHOLD = 6; // px of movement before a cell-drag becomes a marquee
  const EDGE_ZONE = 40; // px from viewport top/bottom that auto-scrolls
  const EDGE_STEP = 7; // px scrolled per frame while in the edge zone

  /** Marquee rectangle in canvas (content) coordinates; null when idle. */
  let marquee = $state<{ x: number; y: number; w: number; h: number } | null>(null);

  let drag: {
    pointerId: number;
    startX: number;
    startY: number; // canvas coords, so scrolling mid-drag keeps the origin
    base: Set<number>; // selection to add to (Ctrl held at drag start)
    active: boolean;
    lastX: number;
    lastViewY: number; // viewport-relative, for edge auto-scroll
  } | null = null;
  let edgeRaf = 0;
  // Touch: a pending long-press (before it turns into a marquee).
  let longPressTimer: ReturnType<typeof setTimeout> | null = null;
  let touchStart: { x: number; y: number } | null = null;

  function cancelLongPress() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
    touchStart = null;
  }

  /** Cells are a uniform grid — geometry replaces DOM hit-testing. Returns
   *  null off-grid (scrollbar) or the hit index (may be out of range). */
  function hitTest(e: { clientX: number; clientY: number }): { x: number; y: number; index: number; onCell: boolean } | null {
    if (!viewport) return null;
    const rect = viewport.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const viewY = e.clientY - rect.top;
    if (x >= viewport.clientWidth) return null; // scrollbar, not the grid
    const y = viewY + viewport.scrollTop;
    const col = Math.floor((x - padX) / CELL);
    const index = Math.floor((y - MARGIN_Y) / CELL) * cols + col;
    const onCell = col >= 0 && col < cols && index >= 0 && index < items.length;
    return { x, y, index, onCell };
  }

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0 || !viewport) return; // marquee/selection: primary button only
    const hit = hitTest(e);
    if (!hit) return;
    const { x, y, index, onCell } = hit;
    const viewY = y - viewport.scrollTop;
    if (onCell) {
      if (e.shiftKey) session.rangeSelect(index, e.ctrlKey);
      else if (e.ctrlKey) session.toggleSelect(index);
      else session.selectOnly(index);
    }
    if (e.shiftKey) return; // Shift is range-select; never starts a marquee

    const beginMarquee = (active: boolean) => {
      drag = {
        pointerId: e.pointerId,
        startX: x,
        startY: y,
        base: e.ctrlKey ? new Set(session.selectedIds) : new Set(),
        active,
        lastX: x,
        lastViewY: viewY,
      };
      viewport!.setPointerCapture(e.pointerId);
      if (drag.active) applyMarquee(x, y);
    };

    if (e.pointerType === "touch") {
      // A plain touch drag scrolls the grid; only a long-press (finger held
      // still) starts a marquee. The cell tap above already set the selection.
      cancelLongPress();
      touchStart = { x: e.clientX, y: e.clientY };
      longPressTimer = setTimeout(() => {
        longPressTimer = null;
        touchStart = null;
        beginMarquee(true);
      }, LONG_PRESS_MS);
      return;
    }

    // Mouse/pen: empty space starts the marquee at once (a plain click there
    // clears the selection); dragging off a cell needs the movement threshold.
    beginMarquee(!onCell);
  }

  function onPointerMove(e: PointerEvent) {
    // Still deciding tap-vs-long-press: any real movement means a scroll, so
    // drop the pending marquee and let the browser scroll.
    if (longPressTimer && touchStart) {
      if (
        Math.abs(e.clientX - touchStart.x) > DRAG_THRESHOLD ||
        Math.abs(e.clientY - touchStart.y) > DRAG_THRESHOLD
      ) {
        cancelLongPress();
      }
      return;
    }
    if (!drag || e.pointerId !== drag.pointerId || !viewport) return;
    const rect = viewport.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const viewY = e.clientY - rect.top;
    const y = viewY + viewport.scrollTop;
    if (!drag.active) {
      if (
        Math.abs(x - drag.startX) < DRAG_THRESHOLD &&
        Math.abs(y - drag.startY) < DRAG_THRESHOLD
      ) {
        return;
      }
      drag.active = true;
    }
    drag.lastX = x;
    drag.lastViewY = viewY;
    applyMarquee(x, y);
    if (!edgeRaf) edgeRaf = requestAnimationFrame(edgeScroll);
  }

  /** Gentle auto-scroll while the marquee pointer sits near an edge. */
  function edgeScroll() {
    edgeRaf = 0;
    if (!drag?.active || !viewport) return;
    let dy = 0;
    if (drag.lastViewY < EDGE_ZONE) dy = -EDGE_STEP;
    else if (drag.lastViewY > height - EDGE_ZONE) dy = EDGE_STEP;
    if (dy !== 0) {
      viewport.scrollTop += dy;
      scrollTop = viewport.scrollTop;
      applyMarquee(drag.lastX, drag.lastViewY + viewport.scrollTop);
      edgeRaf = requestAnimationFrame(edgeScroll);
    }
  }

  function applyMarquee(x: number, y: number) {
    if (!drag) return;
    const x0 = Math.min(drag.startX, x);
    const x1 = Math.max(drag.startX, x);
    const y0 = Math.min(drag.startY, y);
    const y1 = Math.max(drag.startY, y);
    marquee = { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };

    const next = new Set(drag.base);
    const c0 = Math.max(0, Math.floor((x0 - padX) / CELL));
    const c1 = Math.min(cols - 1, Math.floor((x1 - padX) / CELL));
    const r0 = Math.max(0, Math.floor((y0 - MARGIN_Y) / CELL));
    const r1 = Math.min(totalRows - 1, Math.floor((y1 - MARGIN_Y) / CELL));
    for (let r = r0; r <= r1; r++) {
      for (let c = c0; c <= c1; c++) {
        const i = r * cols + c;
        if (i < items.length) next.add(items[i].id);
      }
    }
    session.selectedIds = next;
  }

  /** Double-click a cell to open the loupe. Pointer capture (set in
   *  onPointerDown) redirects click/dblclick hit-testing to .viewport, so
   *  this can't live on the .cell element — recompute the hit cell instead. */
  function onDblClick(e: MouseEvent) {
    const hit = hitTest(e);
    if (hit?.onCell) view.mode = "viewer";
  }

  function endDrag(e: PointerEvent) {
    cancelLongPress(); // clear a pending touch long-press (this was a tap/scroll)
    if (!drag || e.pointerId !== drag.pointerId) return;
    if (viewport?.hasPointerCapture(e.pointerId)) viewport.releasePointerCapture(e.pointerId);
    drag = null;
    marquee = null;
    if (edgeRaf) {
      cancelAnimationFrame(edgeRaf);
      edgeRaf = 0;
    }
  }
</script>

<div
  class="viewport"
  role="grid"
  aria-label="Photo grid"
  tabindex="-1"
  bind:this={viewport}
  bind:clientWidth={width}
  bind:clientHeight={height}
  onscroll={onScroll}
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={endDrag}
  onpointercancel={endDrag}
  ondblclick={onDblClick}
>
  <div class="canvas" bind:this={canvasEl} style="height:{totalRows * CELL + MARGIN_Y * 2}px">
    {#each visible as v}
      <div
        class="cell"
        class:focused={v.index === session.focusedIndex}
        class:selected={session.selectedIds.has(v.item.id)}
        style="transform: translate({v.x}px, {v.y}px); width:{CELL}px; height:{CELL}px"
        role="button"
        tabindex="-1"
      >
        <div class="frame" style:--label-color={v.item.label ? labelColors[v.item.label] : "transparent"}>
          {#if v.item.kind === 2}
            <!-- preload=metadata shows the first frame; only ~30 cells live -->
            <video src={videoUrl(v.item)} preload="metadata" muted></video>
            <span class="chip video"><Play size={10} /></span>
          {:else if v.item.thumbFailed}
            <div class="unreadable" title="{v.item.name}.{v.item.ext} — couldn't be decoded">
              <FileWarning size={22} />
              <span>{v.item.ext.toUpperCase()}</span>
            </div>
          {:else}
            <img
              src={thumbUrl(v.item)}
              alt=""
              decoding="async"
              draggable="false"
              loading="eager"
            />
          {/if}
          {#if session.mirrorMode && v.item.groupSize > 1}
            <span class="chip pair" class:split={v.item.decoupled}>
              {#if v.item.decoupled}<Scissors size={10} /><span>SPLIT</span>{:else}RAW+JPG{/if}
            </span>
          {:else if v.item.kind === 0}
            <span class="chip raw">RAW</span>
          {/if}
          {#if session.pendingDeleteIds.has(v.item.id)}
            <span class="badge pending" title="Queued for deletion"><Trash2 size={12} /></span>
          {:else if v.item.flag !== 0}
            <span class="badge" class:pick={v.item.flag === 1} class:reject={v.item.flag === -1}>
              {#if v.item.flag === 1}<Check size={12} />{:else}<X size={12} />{/if}
            </span>
          {/if}
          {#if v.item.rating > 0}
            <span class="stars">{"★".repeat(v.item.rating)}</span>
          {/if}
          {#if v.item.tagIds.length > 0}
            <span class="tags">
              {#each v.item.tagIds.slice(0, 4) as tagId}
                <span
                  class="tagdot"
                  style="background: {tags.byId.get(tagId)?.color ?? '#888'}"
                  title={tags.byId.get(tagId)?.name}
                ></span>
              {/each}
            </span>
          {/if}
        </div>
        {#if session.showNames}
          <span class="name">{v.item.name}.{v.item.ext}</span>
        {/if}
      </div>
    {/each}
    {#if marquee}
      <div
        class="marquee"
        style="transform: translate({marquee.x}px, {marquee.y}px); width:{marquee.w}px; height:{marquee.h}px"
      ></div>
    {/if}
  </div>
</div>

<style>
  .viewport {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    contain: strict;
    /* Let the browser handle vertical scroll; a long-press marquee takes over
       via pointer capture. */
    touch-action: pan-y;
  }

  .canvas {
    position: relative;
  }

  .cell {
    position: absolute;
    top: 0;
    left: 0;
    display: flex;
    flex-direction: column;
    padding: 6px;
    box-sizing: border-box;
    gap: 4px;
    border-radius: 8px;
    user-select: none; /* marquee drags must not select label text */
  }

  .cell.focused {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
    background: rgba(var(--accent-rgb), 0.08);
  }

  .cell.selected {
    background: rgba(var(--accent-rgb), 0.16);
    box-shadow: inset 0 0 0 1px rgba(var(--accent-rgb), 0.55);
  }

  .frame {
    position: relative;
    flex: 1;
    min-height: 0;
    background: var(--hover);
    border-radius: 6px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    border-bottom: 3px solid var(--label-color);
    /* Portrait images are height-constrained and would sit flush on the
       frame's bottom edge; keep a small constant gap below any image. */
    padding-bottom: 4px;
  }

  .marquee {
    position: absolute;
    top: 0;
    left: 0;
    border: 1px dashed var(--accent);
    background: rgba(var(--accent-rgb), 0.13);
    pointer-events: none;
    z-index: 2;
  }

  img,
  video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    user-select: none;
  }

  /* Shown instead of a thumbnail when the source couldn't be decoded. */
  .unreadable {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    color: #6a6a72;
    user-select: none;
  }

  .unreadable span {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.03em;
  }

  .chip.video {
    left: auto;
    right: 4px;
    bottom: 4px;
    top: auto;
  }

  .chip {
    position: absolute;
    top: 4px;
    left: 4px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.55);
    color: #ddd;
  }

  .chip.pair {
    color: #8fd0ff;
  }

  .chip.pair.split {
    color: #ffb86b;
  }

  .badge {
    position: absolute;
    top: 4px;
    right: 4px;
    display: inline-flex;
    align-items: center;
    font-size: 12px;
    padding: 2px 4px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.55);
  }

  .badge.pick {
    color: #6be675;
  }

  .badge.reject {
    color: #ff6b6b;
  }

  .badge.pending {
    outline: 1px solid #ffb86b;
  }

  .stars {
    position: absolute;
    bottom: 4px;
    left: 6px;
    color: #ffd166;
    font-size: 12px;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
  }

  .tags {
    position: absolute;
    bottom: 6px;
    right: 6px;
    display: flex;
    gap: 3px;
  }

  .tagdot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    box-shadow: 0 0 2px rgba(0, 0, 0, 0.8);
  }

  .name {
    font-size: 11px;
    opacity: 0.65;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    text-align: center;
  }
</style>
