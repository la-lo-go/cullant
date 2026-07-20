<script lang="ts">
  import { thumbUrl, displayDims, type ItemLite } from "../api";
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import { settings } from "../stores/settings.svelte";
  import { tags } from "../stores/tags.svelte";
  import { view } from "../stores/view.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Play from "@lucide/svelte/icons/play";
  import Film from "@lucide/svelte/icons/film";
  import FileWarning from "@lucide/svelte/icons/file-warning";
  import Loader from "@lucide/svelte/icons/loader";
  import { edgeBounce } from "../anim";

  let { items }: { items: ItemLite[] } = $props();

  const OVERSCAN_ROWS = 2;
  const LONG_PRESS_MS = 400; // touch: hold this long to start a marquee
  const MARGIN_Y = 14; // breathing room above the first row and below the last
  // Inter-cell gap: the pitch (CELL) still tiles edge-to-edge for all hit-test
  // and marquee math, but each cell's visual box is inset by GAP so adjacent
  // focus/selection outlines never touch. Kept out of the pitch math on purpose.
  const GAP = 6;
  // Selected cells shrink a touch further — extra breathing room so a block of
  // adjacent selections never reads as one solid blue mass. Computed as a
  // plain extra inset (added to the existing GAP/2 offset, subtracted twice
  // from the width/height) rather than a CSS `scale`: composing `scale` with
  // the cell's own `transform: translate(...)` positioning read as selected
  // cells overlapping their neighbors instead of shrinking cleanly in place.
  const SELECTED_INSET = 5;

  // A small pill (bottom-right, over the cells) reporting whatever background
  // work is in flight: scanning, then thumbnail generation, then previews. All
  // status lives here — the top bar never shows these messages.
  const bgStatus = $derived.by(() => {
    if (catalog.scanning) return `Scanning… ${catalog.scanFound || 0}`;
    const t = catalog.thumbProgress;
    if (t.total > 0) return `Thumbnails ${t.done} / ${t.total}`;
    const p = catalog.previewProgress;
    if (p.total > 0) return `Previews ${p.done} / ${p.total}`;
    const v = catalog.videoProgress;
    if (v.total > 0) return `Video thumbnails ${v.done} / ${v.total}`;
    return null;
  });

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  // Video ids whose pregenerated poster couldn't be served (ffmpeg absent or
  // the clip was undecodable → 404). Those cells fall back to a live <video>.
  let posterFailed = $state<Set<number>>(new Set());
  function markPosterFailed(id: number) {
    if (posterFailed.has(id)) return;
    posterFailed = new Set(posterFailed).add(id);
  }

  // Ids whose thumbnail has actually painted. Until then a subtle skeleton fills
  // the cell — the grid opens before thumbnails are pregenerated, so many cells
  // are briefly empty and fill in progressively. Lives in the catalog store so it
  // survives this component unmounting (loupe/compare) and remounting, which must
  // not re-skeleton and re-request thumbnails that already exist.
  const loaded = catalog.thumbLoaded;
  // Ids whose full loupe preview has been generated. A photo whose thumbnail has
  // painted but whose preview is not here yet — while the preview pass runs —
  // shows a small "generating preview" spinner.
  const previewReady = catalog.previewReady;

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
  const BASE_CELL = $derived(width > 0 && width < 520 ? 116 : 188);
  // Never collapse below two columns: on very narrow viewports keep 2 columns
  // and shrink the cells to fit instead. Wider viewports keep the base pitch.
  const MIN_COLS = 2;
  const cols = $derived(width > 0 ? Math.max(MIN_COLS, Math.floor(width / BASE_CELL)) : 1);
  const CELL = $derived(cols * BASE_CELL <= width ? BASE_CELL : Math.max(1, Math.floor(width / cols)));
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

  // Keep the focused cell in view: on keyboard navigation, and once on mount
  // (e.g. returning from the loupe/compare, so the grid lands back on the row
  // of whatever was shown there). Skipped when nothing is focused (-1) — a
  // selection in progress deliberately drops focus (see beginMarquee), and
  // scrolling to a stale/negative row here would yank the view away from the
  // selection the user is mid-drag on. Also skipped until `height` reports a
  // real measurement (bind:clientHeight arrives via ResizeObserver, a tick
  // after mount) — computing against the stale 0 would scroll to the wrong
  // spot on the very first run, right after VirtualGrid remounts.
  $effect(() => {
    if (!viewport || session.focusedIndex === -1 || height === 0) return;
    const row = Math.floor(session.focusedIndex / cols);
    const top = MARGIN_Y + row * CELL;
    const bottom = top + CELL;
    if (top < viewport.scrollTop) {
      viewport.scrollTo({ top });
    } else if (bottom > viewport.scrollTop + height) {
      viewport.scrollTo({ top: bottom - height });
    }
  });

  // Only the visible window is materialized. The #each below is keyed by file
  // id so each thumbnail owns a stable <img>: scrolling adds/removes cells
  // instead of reassigning `src` on reused nodes. (Positional reuse recycled
  // fewer nodes but reassigned `src` mid-scroll, so a node briefly kept showing
  // its previous image until the new one decoded — a rapid flicker on fast
  // scroll, worst travelling toward index 0.)
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
  // Touch tap-vs-scroll, recorded on touchstart. A release under TAP_SLOP with
  // `moved` still false is a tap (select + open the loupe); any larger movement
  // is a scroll and leaves focus/selection untouched. Cleared when a marquee begins.
  const TAP_SLOP = 10;
  let touchTap: { index: number; onCell: boolean; x: number; y: number; moved: boolean } | null =
    null;

  function cancelLongPress() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
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
      if (drag.active) {
        // A genuine multi-select drag starting: drop any single focused cell
        // (it may be scrolled off-screen, and the scroll-into-view effect
        // would otherwise yank the view there mid-drag) rather than tracking
        // a "focus" concept that no longer applies to a marquee.
        session.focusedIndex = -1;
        applyMarquee(x, y);
      }
    };

    if (e.pointerType === "touch") {
      // Defer to pointerup: a tap selects + opens the loupe, a swipe scrolls and
      // must not disturb focus/selection. A long-press (finger held still past
      // LONG_PRESS_MS without scrolling) starts a marquee instead. Selection is
      // NOT set here, so the initial press of a scroll never jumps the focus.
      cancelLongPress();
      touchTap = { index, onCell, x: e.clientX, y: e.clientY, moved: false };
      longPressTimer = setTimeout(() => {
        longPressTimer = null;
        touchTap = null; // became a marquee, not a tap
        beginMarquee(true);
      }, LONG_PRESS_MS);
      return;
    }

    // Mouse/pen: select immediately, then arm the marquee.
    if (onCell) {
      if (e.shiftKey) session.rangeSelect(index, e.ctrlKey);
      else if (e.ctrlKey) session.toggleSelect(index);
      else session.selectOnly(index);
    }
    if (e.shiftKey) return; // Shift is range-select; never starts a marquee

    // Empty space starts the marquee at once (a plain click there clears the
    // selection); dragging off a cell needs the movement threshold.
    beginMarquee(!onCell);
  }

  function onPointerMove(e: PointerEvent) {
    // Touch, before any marquee: decide tap-vs-scroll. Movement past TAP_SLOP
    // marks a scroll — the pending long-press is dropped and the release will
    // neither select nor open; the browser keeps scrolling.
    if (touchTap && !drag) {
      if (
        !touchTap.moved &&
        (Math.abs(e.clientX - touchTap.x) > TAP_SLOP ||
          Math.abs(e.clientY - touchTap.y) > TAP_SLOP)
      ) {
        touchTap.moved = true;
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
      // Same reasoning as beginMarquee: a click-then-drag past the threshold
      // just became a genuine marquee, so drop the single focused cell.
      session.focusedIndex = -1;
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
    // Touch tap: finger lifted (pointerup, not a cancelled/scrolled gesture)
    // without crossing TAP_SLOP → select the cell and open the loupe. A single
    // tap opens the preview. Scrolls arrive as pointercancel or with `moved`
    // set and are ignored here.
    if (touchTap) {
      const tap = touchTap;
      touchTap = null;
      cancelLongPress();
      if (e.type !== "pointercancel" && !tap.moved && tap.onCell) {
        if (session.selectedIds.size > 0) {
          // A selection is already active (started via long-press): taps toggle
          // membership instead of opening, so you can build a multi-selection one
          // tap at a time.
          session.toggleSelect(tap.index);
        } else {
          session.selectOnly(tap.index);
          // Note the tap-open so ZoomImage can ignore the second tap of a
          // habitual double-tap-to-open (which would otherwise zoom on arrival).
          view.markOpenedFromGrid();
          view.mode = "viewer";
        }
      }
      return;
    }
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

<div class="grid-root">
  <div
    class="viewport"
    id="photo-grid-scroll"
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
    {#each visible as v (v.item.id)}
      {@const selected = session.selectedIds.has(v.item.id)}
      {@const inset = selected ? SELECTED_INSET : 0}
      {@const dims = displayDims(v.item)}
      {@const portrait = dims !== null && dims.h > dims.w}
      <div
        class="cell"
        class:focused={v.index === session.focusedIndex}
        class:selected
        style="transform: translate({v.x + GAP / 2 + inset}px, {v.y + GAP / 2 + inset}px); width:{CELL - GAP - inset * 2}px; height:{CELL - GAP - inset * 2}px"
        role="button"
        tabindex="-1"
      >
        <div
          class="frame"
          class:loading={!loaded.has(v.item.id) &&
            !v.item.thumbFailed &&
            !(v.item.kind === 2 && posterFailed.has(v.item.id))}
        >
          <!-- Sized to the item's REAL aspect ratio when portrait (instead of
               filling the square frame and cropping), so every badge/chip/label
               below — all positioned relative to THIS box, not .frame — stays
               within the actual visible photo instead of spilling into the
               empty letterbox gutters. Landscape/unknown-dims items fill the
               frame exactly (unchanged from before), cropped via object-fit. -->
          <div
            class="photo"
            class:queued={settings.dimQueuedDeletes &&
              (v.item.flag === -1 || session.pendingDeleteIds.has(v.item.id))}
            style={portrait && dims ? `width:auto; aspect-ratio:${dims.w}/${dims.h}` : ""}
          >
            {#if v.item.kind === 2}
              {#if posterFailed.has(v.item.id)}
                <!-- No pregenerated poster (ffmpeg absent / undecodable): a
                     neutral, on-brand placeholder that still reads as a video,
                     instead of a live <video> (heavy on mobile, and it usually
                     just paints black when the poster couldn't be made). -->
                <div class="no-poster" title="{v.item.name}.{v.item.ext}">
                  <Film size={22} />
                  <span>{v.item.ext.toUpperCase()}</span>
                </div>
              {:else}
                <img
                  src={thumbUrl(v.item)}
                  alt=""
                  decoding="async"
                  draggable="false"
                  loading="eager"
                  onload={() => loaded.add(v.item.id)}
                  onerror={() => markPosterFailed(v.item.id)}
                />
              {/if}
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
                onload={() => loaded.add(v.item.id)}
              />
            {/if}
            {#if v.item.kind !== 2 && !v.item.thumbFailed && loaded.has(v.item.id) && !previewReady.has(v.item.id) && catalog.previewProgress.total > 0}
              <span class="preview-spin" title="Generating full preview…">
                <Loader size={12} />
              </span>
            {/if}
            {#if v.item.label}
              <span class="label-bar" style:border-color={labelColors[v.item.label]}></span>
            {/if}
            {#if session.mirrorMode && v.item.groupSize > 1}
              <span class="chip pair" class:split={v.item.decoupled}>
                {#if v.item.decoupled}<Scissors size={10} /><span>SPLIT</span>{:else}RAW+JPG{/if}
              </span>
            {:else if v.item.kind === 0}
              <span class="chip raw">RAW</span>
            {/if}
            {#if session.pendingDeleteIds.has(v.item.id)}
              <span class="badge pending" title="Queued for deletion"><X size={12} /></span>
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
  <!-- Native bar hidden on .viewport below — matches the folder tree/filmstrip
       convention (no 8px carved out of the grid's own width for a scrollbar
       gutter); this floats over the content instead. -->
  <OverlayScrollbar
    orientation="vertical"
    viewport={height}
    content={totalRows * CELL + MARGIN_Y * 2}
    position={scrollTop}
    controls="photo-grid-scroll"
    onSeek={(pos) => {
      if (viewport) viewport.scrollTop = pos;
    }}
  />
  {#if bgStatus}
    <div class="loading-pill" role="status" aria-live="polite">
      <span class="spin"><Loader size={13} /></span>
      <span>{bgStatus}</span>
    </div>
  {/if}
</div>

<style>
  /* Positioned wrapper so the loading pill can float over the scrolling grid
     without scrolling away with the cells. */
  .grid-root {
    position: relative;
    flex: 1;
    display: flex;
    min-height: 0;
    min-width: 0;
  }

  .viewport {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    contain: strict;
    /* Let the browser handle vertical scroll; a long-press marquee takes over
       via pointer capture. */
    touch-action: pan-y;
    /* Native bar hidden — same treatment as the folder tree and filmstrip;
       the OverlayScrollbar sibling above renders the visible thumb instead. */
    scrollbar-width: none;
  }

  .viewport::-webkit-scrollbar {
    display: none;
  }

  .loading-pill {
    position: absolute;
    right: 12px;
    bottom: 12px;
    z-index: 5;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35);
    font-size: 11px;
    line-height: 1;
    color: inherit;
    pointer-events: none;
  }

  .loading-pill .spin {
    display: inline-flex;
    animation: pill-spin 1s linear infinite;
  }

  @keyframes pill-spin {
    to {
      transform: rotate(360deg);
    }
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
    /* Animates the selection shrink (width/height/position all move together
       by SELECTED_INSET). Harmless elsewhere: a mounted cell's own x/y is
       invariant under scrolling (only which cells are visible changes), so
       this never fires on scroll — only on selection toggling, or a column
       count change (screen rotation), where the slide is a nice touch too. */
    transition:
      transform 100ms ease-out,
      width 100ms ease-out,
      height 100ms ease-out;
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
    /* No fill: a portrait photo's letterbox gutters (.photo narrower than
       .frame — see below) and the gap while a thumbnail is still decoding just
       show the grid's own dark background instead of a distinct gray box. */
    background: transparent;
    border-radius: 6px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  /* The actual visible-photo box: fills .frame for landscape/square/unknown-
     dims items (cropped via object-fit: cover below), but for a portrait item
     is sized to its REAL aspect ratio (inline style, computed from the item's
     displayed w/h) instead of the full square frame — full height, auto width,
     so it's flush top/bottom and pillarboxed left/right without cropping.
     Every chip/badge/stars/tags/label-color below is positioned relative to
     THIS box (it's their nearest `position: relative` ancestor), which is
     exactly what keeps them inside the actual photo instead of spilling into
     the empty letterbox gutters. */
  .photo {
    position: relative;
    width: 100%;
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    border-radius: inherit;
  }

  /* Marked for deletion (reject flag or queued delete): dim the image itself,
     not .photo — opacity on the wrapper would wash out the red-X badge (and
     the other chips), which must stay fully readable. Same treatment as
     Filmstrip.svelte. */
  .photo.queued img {
    opacity: 0.4;
    filter: grayscale(35%);
  }

  /* Label-color indicator: a colored strip flush along the photo's bottom
     edge — full-width, side to side, its top edge curving up at both ends
     with the photo's corner rounding. It's the overlay element's OWN bottom
     border (inset: 0 + box-sizing: border-box), which reproduces the old
     `.frame.labeled` border look without its drawback: a border on .photo
     itself would participate in the box model and either overflow .photo's
     100%/100% box or shrink the image to make room for itself. Sits BELOW
     .stars/.tags (bottom: 9px), so the two never overlap. */
  .label-bar {
    position: absolute;
    inset: 0;
    box-sizing: border-box;
    border-bottom: 3px solid;
    border-radius: inherit;
    pointer-events: none;
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

  img {
    /* .photo is already sized to the exact box the image should occupy (the
       full frame for landscape/unknown dims, or the true aspect-ratio box for
       portrait) — cover always fills it exactly, with no cropping in the
       portrait case since the box ratio already matches the image's. */
    width: 100%;
    height: 100%;
    object-fit: cover;
    user-select: none;
  }

  /* Subtle shimmer while a cell's thumbnail is still being generated/decoded.
     It's the frame's own background (behind .photo and its chips), so chips
     stay on top and a finished portrait cell shows the grid's dark background
     through its letterbox gutters, not a gray box. */
  .frame.loading {
    background: linear-gradient(
      100deg,
      var(--surface-2) 30%,
      var(--hover) 50%,
      var(--surface-2) 70%
    );
    background-size: 200% 100%;
    animation: cell-shimmer 2.4s ease-in-out infinite;
  }

  @keyframes cell-shimmer {
    from {
      background-position: 200% 0;
    }
    to {
      background-position: -200% 0;
    }
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

  /* Video with no generated poster (ffmpeg missing / undecodable clip). Reads
     as a deliberate video tile, not an error: a filled dark box + film glyph +
     extension, with the Play chip still on top. */
  .no-poster {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    background: var(--surface-2);
    color: #8a8a93;
    user-select: none;
  }

  .no-poster span {
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
    color: #ff6b6b;
  }

  /* bottom: 9px (not flush) — clears the label-bar below it (see .label-bar;
     that occupies the photo's bottom 0-3px), so the two never overlap. */
  .stars {
    position: absolute;
    bottom: 9px;
    left: 6px;
    color: #ffd166;
    font-size: 12px;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
  }

  .tags {
    position: absolute;
    bottom: 9px;
    right: 6px;
    display: flex;
    gap: 3px;
  }

  /* "Full preview still generating" hint. Bottom-right is free for photos during
     the preview pass (the video chip is video-only; rating/flag/tag chips get
     added later, while culling). */
  .preview-spin {
    position: absolute;
    bottom: 4px;
    right: 4px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 3px;
    border-radius: 50%;
    color: #fff;
    background: rgba(0, 0, 0, 0.5);
    box-shadow: 0 0 2px rgba(0, 0, 0, 0.8);
    animation: pill-spin 1s linear infinite;
    pointer-events: none;
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
