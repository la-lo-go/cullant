<script lang="ts">
  import { thumbUrl, displayDims, containFit, type ItemLite } from "../api";
  import { describeHalf, session } from "../stores/session.svelte";
  import { settings } from "../stores/settings.svelte";
  import { tags } from "../stores/tags.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
  import Scissors from "@lucide/svelte/icons/scissors";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Play from "@lucide/svelte/icons/play";
  import Film from "@lucide/svelte/icons/film";
  import Layers from "@lucide/svelte/icons/layers";

  // `companionIndex` is compare's second photo: marked too, but never as
  // strongly as the focused one, which is what the action bar acts on.
  let {
    items,
    companionIndex = null,
  }: { items: ItemLite[]; companionIndex?: number | null } = $props();

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  const CELL = 96;
  const OVERSCAN = 6;

  // Video ids whose pregenerated poster couldn't be served (no frame extractor
  // on this platform, or the clip was undecodable → 404). Those cells fall back
  // to a Film-glyph tile, the same as VirtualGrid — never a live <video>
  // (heavy: scrolling clips would spin up many decoders, and a metadata failure
  // just paints a blank box).
  let posterFailed = $state<Set<number>>(new Set());
  function markPosterFailed(id: number) {
    if (posterFailed.has(id)) return;
    posterFailed = new Set(posterFailed).add(id);
  }
  // .cell's own padding (see its CSS) — the box object-fit: contain fits into.
  const PAD_X = 3;
  const PAD_Y = 8;
  const AVAIL_W = CELL - PAD_X * 2;
  // Absolute floor/ceiling for the drag-resize (see onResizeMove) — the
  // effective ceiling is the lower of MAX_H and contentMaxH below.
  const MIN_H = 72;
  const MAX_H = 320;

  let strip = $state<HTMLDivElement | null>(null);
  let scrollLeft = $state(0);
  let width = $state(0);

  // Tallest the strip ever needs to be: the height at which even the most-
  // portrait thumbnail stops growing (each thumbnail is contain-fit, so its
  // rendered height tops out at AVAIL_W * h/w — width-bound). Dragging past
  // that only adds empty space above/below every cell. With no known dims
  // there's no content cap, just the absolute MAX_H.
  // Memoized on the items reference: the max aspect ratio only changes when the
  // item set (hence the array reference) changes. A flag/sort/filter remap that
  // yields the same reference reuses the cached value instead of re-scanning the
  // whole catalog. Dimensions only become known via a catalog refresh, which
  // also replaces the array, so a stale reference can never hide a dims change.
  let cachedItemsRef: ItemLite[] | null = null;
  let cachedContentMaxH = MAX_H;
  const contentMaxH = $derived.by(() => {
    if (items === cachedItemsRef) return cachedContentMaxH;
    let maxRatio = 0;
    for (const item of items) {
      const dims = displayDims(item);
      if (dims && dims.w > 0) maxRatio = Math.max(maxRatio, dims.h / dims.w);
    }
    cachedItemsRef = items;
    cachedContentMaxH =
      maxRatio <= 0
        ? MAX_H
        : Math.min(MAX_H, Math.max(MIN_H, Math.ceil(AVAIL_W * maxRatio) + PAD_Y * 2));
    return cachedContentMaxH;
  });
  // Rendered height: the persisted preference clamped to what the current
  // items can actually fill (a tall preference saved from a portrait-heavy
  // catalog must not leave dead space in a landscape-only one).
  const stripH = $derived(Math.max(MIN_H, Math.min(session.filmstripHeight, contentMaxH)));
  // Reactive: the strip is user-resizable (drag its top edge), and unlike the
  // (square, fixed-height) grid cell, a filmstrip cell's height is the OTHER
  // free axis besides width — object-fit: contain can bind on either one
  // depending on the photo's own aspect ratio, so badges anchored to a fixed
  // offset from the CELL (rather than the actual rendered photo) drift away
  // from the image whenever height is the non-binding axis.
  const availH = $derived(Math.max(0, stripH - PAD_Y * 2));

  const first = $derived(Math.max(0, Math.floor(scrollLeft / CELL) - OVERSCAN));
  const last = $derived(
    Math.min(items.length, Math.ceil((scrollLeft + width) / CELL) + OVERSCAN)
  );

  // Keyed by file id in the template so each frame keeps a stable <img>:
  // scrolling adds/removes cells instead of reassigning `src` on reused nodes,
  // which previously left a node showing its old thumbnail until the new one
  // decoded — a rapid flicker on fast (right-to-left) scroll. `photoW`/`photoH`
  // are the exact contain-fit box for the item's real aspect ratio (or the
  // full available box, when dims aren't known yet) — see AVAIL_W/availH.
  const visible = $derived.by(() => {
    const out: { item: ItemLite; index: number; x: number; photoW: number; photoH: number }[] =
      [];
    for (let i = first; i < last; i++) {
      const item = items[i];
      const fit = containFit(displayDims(item), AVAIL_W, availH);
      out.push({ item, index: i, x: i * CELL, photoW: fit.w, photoH: fit.h });
    }
    return out;
  });

  // Keep the focused frame centered as the user arrows through the shoot.
  // A single step scrolls smoothly; while stepping fast (holding an arrow key)
  // we fall back to instant so recentring never lags behind the selection.
  const RAPID_STEP_MS = 180;
  let lastRecentre = 0;
  let hasCentered = false;
  // Lock-carousel: `performance.now()` of the last scroll the user drove while
  // the mode is on, so the recentre effect below doesn't counter-scroll (fight)
  // the drag that is itself moving the focus.
  let lockScrollTs = 0;
  // True between asking for a recentre and the scroll event it produces.
  //
  // Without it, lock-carousel loses a keypress at either end of the strip: the
  // last few photos cannot be centred, so the browser CLAMPS the recentre, and
  // the clamped position reads back as an earlier cell — which the handler
  // below then wrote to the focus, undoing the step that asked for it. The
  // second press worked only because the strip was already at the limit, so no
  // scroll event fired at all. The strip must follow the focus here, never the
  // other way round.
  let selfScroll = false;

  // Scroll handler: track the position and, in lock-carousel mode, drive the
  // focused photo from whichever cell is centered — so moving the strip moves
  // the loupe. Independent scrolling (mode off) just tracks the position.
  function onStripScroll() {
    if (!strip) return;
    scrollLeft = strip.scrollLeft;
    if (selfScroll) {
      selfScroll = false;
      return;
    }
    if (settings.lockCarousel && width > 0 && items.length > 0) {
      lockScrollTs = performance.now();
      const centered = Math.round((scrollLeft + width / 2 - CELL / 2) / CELL);
      const idx = Math.max(0, Math.min(items.length - 1, centered));
      if (idx !== session.focusedIndex) {
        session.focusedIndex = idx;
        session.selectionAnchor = idx;
      }
    }
  }
  // The strip div is destroyed/recreated every time the filmstrip is hidden
  // and shown again (the {#if} above swaps it for the peek button), so track
  // its identity: a freshly (re)mounted strip must always center instantly,
  // never replaying a leftover "smooth" from before it was last hidden.
  let lastStripEl: HTMLDivElement | null = null;
  $effect(() => {
    if (!strip || width === 0) return;
    if (strip !== lastStripEl) {
      lastStripEl = strip;
      hasCentered = false;
    }
    const focused = session.focusedIndex; // tracked for reactivity
    // In lock-carousel mode a focus change that came from the user's own strip
    // scroll must not trigger a counter-scroll (it would fight the drag). A focus
    // change from elsewhere (keyboard nav) still recentres, since no recent
    // scroll set lockScrollTs.
    if (settings.lockCarousel && performance.now() - lockScrollTs < 250) {
      hasCentered = true;
      return;
    }
    const target = Math.max(0, focused * CELL - width / 2 + CELL / 2);
    const now = performance.now();
    const rapid = now - lastRecentre < RAPID_STEP_MS;
    lastRecentre = now;
    // Lock-carousel recentres INSTANTLY: a smooth scroll would fire onscroll on
    // intermediate frames whose centered cell reads as an earlier index, driving
    // the focus (and thus the loupe) briefly backwards — the "previous image
    // glitch" — before it lands. A jump has no intermediate frames.
    const instant = !hasCentered || rapid || settings.lockCarousel;
    selfScroll = true;
    strip.scrollTo({ left: target, behavior: instant ? "auto" : "smooth" });
    // A recentre that changes nothing (already there, or clamped to the same
    // limit) fires no scroll event, so disarm on the next frame rather than
    // leaving the flag to swallow the user's next real scroll.
    requestAnimationFrame(() => (selfScroll = false));
    hasCentered = true;
  });

  // Drag the top edge to resize; dragging it below COLLAPSE_AT hides the strip
  // (the peek arrow at the bottom, and the F toggle, bring it back).
  const COLLAPSE_AT = 56;

  let resizing = $state(false);
  // True while the drag is past the collapse threshold: the strip clamps to its
  // minimum and dims, but the hide only commits on pointer release — dragging
  // back out cancels it (standard resize-to-hide behavior).
  let pendingCollapse = $state(false);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onResizeMove(e: PointerEvent) {
    if (!resizing || !strip) return;
    const h = strip.getBoundingClientRect().bottom - e.clientY;
    // Past the threshold only *flags* a pending collapse (strip stays at MIN_H,
    // dimmed) instead of committing it, so dragging back out cancels the hide.
    pendingCollapse = h < COLLAPSE_AT;
    session.filmstripHeight = Math.min(contentMaxH, Math.max(MIN_H, h));
  }

  function endResize(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    if (pendingCollapse) {
      pendingCollapse = false;
      session.setShowFilmstrip(false);
    } else {
      session.setFilmstripHeight(session.filmstripHeight);
    }
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
  <div class="filmstrip" class:pending-collapse={pendingCollapse} style="height: {stripH}px">
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
    <!-- Hide arrow: same size/style as the collapsed peek below, pointing the
         opposite way, overlaid above the thumbnail row (high z-index) instead
         of taking layout space — the same overlay pattern as the folder tree's
         peek arrow. -->
    <button
      class="strip-hide"
      title="Hide filmstrip"
      aria-label="Hide filmstrip"
      onclick={() => session.setShowFilmstrip(false)}
    >
      <ChevronDown size={16} />
    </button>
    <div
      class="strip"
      id="filmstrip-scroll"
      bind:this={strip}
      bind:clientWidth={width}
      onscroll={onStripScroll}
    >
      <div class="canvas" style="width:{items.length * CELL}px">
        {#each visible as v (v.item.id)}
          {@const burst = session.burstPositionAt(v.index)}
          <div
            class="cell"
            class:focused={v.index === session.focusedIndex}
            class:companion={v.index !== session.focusedIndex && v.index === companionIndex}
            style="transform: translateX({v.x}px); width:{CELL}px"
            onpointerdown={(e) => onCellPointerDown(e, v.index)}
            onpointerup={onCellPointerUp}
            onpointercancel={resetTap}
            role="button"
            tabindex="-1"
          >
            <!-- Sized to the item's actual contain-fit box (photoW/photoH,
                 computed from its real aspect ratio — see `visible` above),
                 not the cell: object-fit: contain can bind on either the
                 width or the height axis here (unlike the grid's square
                 cells), so anchoring badges to the cell instead of this box
                 left them a variable, often-large distance from the image
                 whenever height was the slack axis. -->
            <div
              class="photo"
              class:queued={settings.dimQueuedDeletes &&
                (v.item.flag === -1 || session.pendingDeleteIds.has(v.item.id))}
              style="width:{v.photoW}px; height:{v.photoH}px"
            >
              {#if v.item.kind === 2}
                {#if posterFailed.has(v.item.id)}
                  <div class="no-poster" title="{v.item.name}.{v.item.ext}">
                    <Film size={16} />
                    <span>{v.item.ext.toUpperCase()}</span>
                  </div>
                {:else}
                  <img
                    src={thumbUrl(v.item)}
                    alt=""
                    decoding="async"
                    draggable="false"
                    onerror={() => markPosterFailed(v.item.id)}
                  />
                {/if}
                <span class="videobadge"><Play size={9} /></span>
              {:else}
                <img src={thumbUrl(v.item)} alt="" decoding="async" draggable="false" />
              {/if}
              {#if settings.filmstripShowLabel && v.item.label}
                <span class="label-bar" style:border-color={labelColors[v.item.label]}></span>
              {/if}
              {#if burst}
                <span class="burst" title="Shot {burst.position} of a burst of {burst.total}">
                  <Layers size={8} />{burst.position}/{burst.total}
                </span>
              {/if}
              {#if settings.filmstripShowType}
                {#if session.mirrorMode && v.item.groupSize > 1}
                  {@const pair = session.pairHalves(v.item)}
                  <span
                    class="chip"
                    class:split={v.item.decoupled}
                    class:below-burst={burst}
                    title={pair ? pair.halves.map(describeHalf).join(" · ") : undefined}
                  >
                    {#if v.item.decoupled}
                      <Scissors size={8} /><span>SPLIT</span>
                    {:else if pair?.diverged}
                      {#each pair.halves as h, hi (h.id)}
                        {#if hi > 0}<span class="half-sep"></span>{/if}
                        <span class="half" class:struck={h.queuedDelete}>{h.name}</span>
                      {/each}
                    {:else}
                      RAW+JPG
                    {/if}
                  </span>
                {:else if v.item.kind === 0}
                  <span class="chip" class:below-burst={burst}>RAW</span>
                {/if}
              {/if}
              {#if settings.filmstripShowFlag && v.item.flag !== 0}
                <span class="flagbadge" class:pick={v.item.flag === 1} class:reject={v.item.flag === -1}>
                  {#if v.item.flag === 1}<Check size={11} />{:else}<X size={11} />{/if}
                </span>
              {/if}
              {#if settings.filmstripShowRating && v.item.rating > 0}
                <span class="stars">{"★".repeat(v.item.rating)}</span>
              {/if}
              {#if settings.filmstripShowTags && v.item.tagIds.length > 0}
                <span class="tags">
                  {#each v.item.tagIds.slice(0, 3) as tagId}
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
        alwaysVisible
      />
    </div>
  </div>
{:else}
  <button
    class="strip-peek"
    title="Show filmstrip"
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

  /* Dragged past the collapse threshold: a subtle dim signals that releasing
     now will hide the strip (dragging back out cancels). */
  .filmstrip.pending-collapse {
    opacity: 0.6;
    transition: opacity 0.1s;
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

  /* Hide arrow: the mirror of .strip-peek above — same size, hanging from the
     TOP edge instead (pointing down), overlaid above the thumbnails with a
     z-index higher than the resize handle so it stays clickable. */
  .strip-hide {
    position: absolute;
    left: 50%;
    top: 0;
    transform: translateX(-50%);
    z-index: 30;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border);
    border-top: none;
    border-radius: 0 0 6px 6px;
    background: var(--surface);
    color: var(--accent);
    cursor: pointer;
  }

  .strip-hide:hover {
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
    /* Defines the box `visible`'s containFit() sizes .photo into (see AVAIL_W/
       availH in the script) — .photo ends up centered within whatever slack
       that leaves, via the flex centering below. */
    padding: 8px 3px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
  }

  /* In compare the two marks must never read as equal: the action bar only
     ever acts on the focused one, so it gets the solid ring and a lift, and
     the companion gets a dashed, dimmer one. */
  .cell.focused {
    outline: 3px solid var(--accent);
    outline-offset: -3px;
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }

  .cell.companion {
    outline: 2px dashed color-mix(in srgb, var(--accent) 55%, transparent);
    outline-offset: -2px;
  }

  /* .photo is sized (inline style) to the item's real contain-fit box, so
     every badge below anchors to the actual visible photo. */
  .photo {
    position: relative;
    overflow: hidden;
    border-radius: 3px;
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    user-select: none;
  }

  /* Marked for deletion (reject flag or queued delete): dim the image itself,
     not .photo — opacity on the wrapper would wash out the flag badge and
     chips overlaid on the photo. Same treatment as VirtualGrid.svelte. */
  .photo.queued img {
    opacity: 0.4;
    filter: grayscale(35%);
  }

  /* Video with no generated poster (no extractor / undecodable clip): a dark
     film-glyph tile, matching VirtualGrid's fallback. */
  .no-poster {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 2px;
    background: var(--surface-2);
    color: #8a8a93;
    user-select: none;
  }

  .no-poster span {
    font-size: 8px;
    font-weight: 600;
    letter-spacing: 0.03em;
  }

  /* Small play glyph marking a cell as a video (the poster is a still frame). */
  .videobadge {
    position: absolute;
    right: 4px;
    bottom: 4px;
    display: inline-flex;
    align-items: center;
    color: #ddd;
    pointer-events: none;
    filter: drop-shadow(0 1px 1px rgba(0, 0, 0, 0.9));
  }

  /* Label-color indicator: a colored strip flush along the photo's bottom
     edge — full-width, side to side, its top edge curving up at both ends
     with the corner rounding. Same overlay-border technique as
     VirtualGrid.svelte's .label-bar (see that comment for why the border
     lives on an overlay element, not on .photo itself). */
  .label-bar {
    position: absolute;
    inset: 0;
    box-sizing: border-box;
    border-bottom: 3px solid;
    border-radius: inherit;
    pointer-events: none;
  }

  .chip {
    position: absolute;
    top: 4px;
    left: 4px;
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

  /* Halves of a pair that disagree. Same colour as an ordinary pair on purpose:
     it reports the difference, it does not warn about it. */
  .chip .half-sep {
    width: 1px;
    align-self: stretch;
    margin: 1px -1px;
    background: currentColor;
    opacity: 0.4;
  }

  .chip .half.struck {
    text-decoration: line-through;
    opacity: 0.55;
  }

  .chip.split {
    color: #ffb86b;
  }

  /* Second row of the top-left stack: the burst badge owns the corner, same
     order as the grid cell. */
  .chip.below-burst {
    top: 17px;
  }

  /* Burst badge: the grid's pill, sized down for a filmstrip cell. */
  .burst {
    position: absolute;
    top: 4px;
    left: 4px;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: 8px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    padding: 1px 4px;
    border-radius: 999px;
    border: 1px solid var(--border-strong);
    background: rgba(0, 0, 0, 0.55);
    color: #e8e8e8;
    pointer-events: none;
  }

  /* Pick/reject flag badge: a green check / red X — the same visual language
     as the grid's flag badge and both action bars (was a plain colored dot). */
  .flagbadge {
    position: absolute;
    top: 4px;
    right: 4px;
    display: inline-flex;
    pointer-events: none;
    filter: drop-shadow(0 1px 1px rgba(0, 0, 0, 0.9));
  }

  .flagbadge.pick {
    color: #6be675;
  }

  .flagbadge.reject {
    color: #ff6b6b;
  }

  .stars {
    position: absolute;
    bottom: 7px;
    left: 4px;
    color: #ffd166;
    font-size: 8px;
    text-shadow: 0 1px 2px rgba(0, 0, 0, 0.8);
    pointer-events: none;
  }

  .tags {
    position: absolute;
    bottom: 7px;
    right: 4px;
    display: flex;
    gap: 2px;
    pointer-events: none;
  }

  .tagdot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    box-shadow: 0 0 2px rgba(0, 0, 0, 0.8);
  }
</style>
