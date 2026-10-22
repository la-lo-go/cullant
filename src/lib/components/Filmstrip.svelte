<script lang="ts">
  import { LABEL_COLORS } from "../labels";
  import { thumbUrl, displayDims, containFit, type ItemLite } from "../api";
  import { describeHalf, session } from "../stores/session.svelte";
  import { settings } from "../stores/settings.svelte";
  import { tags } from "../stores/tags.svelte";
  import { DOCK_IDLE, dock, isVertical, type Side } from "../stores/dock.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
  import DockGrip from "./DockGrip.svelte";
  import DockGhost from "./DockGhost.svelte";
  import Filmstrip from "./Filmstrip.svelte";
  import { gridThumb } from "../thumbLoader";
  import Scissors from "@lucide/svelte/icons/scissors";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Play from "@lucide/svelte/icons/play";
  import Film from "@lucide/svelte/icons/film";
  import Layers from "@lucide/svelte/icons/layers";

  // `companionIndex` is compare's second photo: marked too, but never as
  // strongly as the focused one, which is what the action bar acts on.
  // `preview` makes a ghost for a drag: the strip as it will look at that side and
  // size, with the focused cell centred. It reads the session and writes nothing.
  let {
    items,
    companionIndex = null,
    preview,
  }: {
    items: ItemLite[];
    companionIndex?: number | null;
    preview?: { side: Side; size: number };
  } = $props();

  const CELL = 96;
  const OVERSCAN = 6;

  // The strip docks on any side. "Main" is the axis it scrolls along, "cross"
  // is its thickness. A vertical strip is a column: the main axis is Y.
  const side = $derived(preview?.side ?? dock.filmstrip);
  const vertical = $derived(isVertical(side));
  // The tab that hides the strip points to the outer edge, the peek tab inward.
  const HIDE_ICON = { bottom: ChevronDown, top: ChevronUp, left: ChevronLeft, right: ChevronRight };
  const PEEK_ICON = { bottom: ChevronUp, top: ChevronDown, left: ChevronRight, right: ChevronLeft };
  const HideIcon = $derived(HIDE_ICON[side]);
  const PeekIcon = $derived(PEEK_ICON[side]);

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
  // It is transposed in a vertical strip.
  const PAD_MAIN = 3;
  const PAD_CROSS = 8;
  const AVAIL_MAIN = CELL - PAD_MAIN * 2;
  // Absolute floor/ceiling for the drag-resize (see onResizeMove) — the
  // effective ceiling is the lower of MAX_T and contentMax below.
  const MIN_T = 72;
  const MAX_T = 320;

  let root = $state<HTMLDivElement | null>(null);
  let strip = $state<HTMLDivElement | null>(null);
  let scrollPos = $state(0);
  let clientW = $state(0);
  let clientH = $state(0);
  const viewLen = $derived(vertical ? clientH : clientW);

  // Thickest the strip ever needs to be: the thickness at which even the most
  // extreme thumbnail stops growing (each thumbnail is contain-fit, so across
  // the strip it tops out at AVAIL_MAIN * the aspect ratio that binds there:
  // h/w for a row, w/h for a column). Dragging past that only adds empty space
  // around every cell. With no known dims there's no content cap, just MAX_T.
  // Memoized on the items reference: the max aspect ratios only change when the
  // item set (hence the array reference) changes. A flag/sort/filter remap that
  // yields the same reference reuses the cached value instead of re-scanning the
  // whole catalog. Dimensions only become known via a catalog refresh, which
  // also replaces the array, so a stale reference can never hide a dims change.
  let cachedItemsRef: ItemLite[] | null = null;
  let cachedRatios = { hOverW: 0, wOverH: 0 };
  const ratios = $derived.by(() => {
    if (items === cachedItemsRef) return cachedRatios;
    let hOverW = 0;
    let wOverH = 0;
    for (const item of items) {
      const dims = displayDims(item);
      if (dims && dims.w > 0 && dims.h > 0) {
        hOverW = Math.max(hOverW, dims.h / dims.w);
        wOverH = Math.max(wOverH, dims.w / dims.h);
      }
    }
    cachedItemsRef = items;
    cachedRatios = { hOverW, wOverH };
    return cachedRatios;
  });
  function contentMax(forVertical: boolean): number {
    const ratio = forVertical ? ratios.wOverH : ratios.hOverW;
    if (ratio <= 0) return MAX_T;
    return Math.min(MAX_T, Math.max(MIN_T, Math.ceil(AVAIL_MAIN * ratio) + PAD_CROSS * 2));
  }
  // Rendered thickness of a strip docked at `at`: the persisted preference
  // clamped to what the current items can actually fill (a thick preference
  // saved from a catalog of extreme ratios must not leave dead space in
  // another one).
  function thicknessFor(at: Side): number {
    const v = isVertical(at);
    const preferred = v ? session.filmstripWidth : session.filmstripHeight;
    return Math.max(MIN_T, Math.min(preferred, contentMax(v)));
  }
  const stripT = $derived(preview?.size ?? thicknessFor(side));
  // Reactive: the strip is user-resizable (drag its inner edge), and unlike the
  // (square, fixed-size) grid cell, a filmstrip cell's cross size is the OTHER
  // free axis besides its length — object-fit: contain can bind on either one
  // depending on the photo's own aspect ratio, so badges anchored to a fixed
  // offset from the CELL (rather than the actual rendered photo) drift away
  // from the image whenever the cross axis is the non-binding one.
  const availCross = $derived(Math.max(0, stripT - PAD_CROSS * 2));

  // A ghost has no scroll: its canvas is shifted so the focused cell is centred.
  const ghostPos = $derived(
    Math.min(
      Math.max(0, items.length * CELL - viewLen),
      Math.max(0, session.focusedIndex * CELL - viewLen / 2 + CELL / 2),
    ),
  );
  const pos = $derived(preview ? ghostPos : scrollPos);
  // A ghost never scrolls, so it draws no cell beyond its box.
  const overscan = $derived(preview ? 0 : OVERSCAN);
  const first = $derived(Math.max(0, Math.floor(pos / CELL) - overscan));
  const last = $derived(Math.min(items.length, Math.ceil((pos + viewLen) / CELL) + overscan));

  // Keyed by file id in the template so each frame keeps a stable <img>:
  // scrolling adds/removes cells instead of reassigning `src` on reused nodes,
  // which previously left a node showing its old thumbnail until the new one
  // decoded — a rapid flicker on fast (right-to-left) scroll. `photoW`/`photoH`
  // are the exact contain-fit box for the item's real aspect ratio (or the
  // full available box, when dims aren't known yet) — see AVAIL_MAIN/availCross.
  const visible = $derived.by(() => {
    const out: { item: ItemLite; index: number; offset: number; photoW: number; photoH: number }[] =
      [];
    for (let i = first; i < last; i++) {
      const item = items[i];
      const dims = displayDims(item);
      const fit = vertical
        ? containFit(dims, availCross, AVAIL_MAIN)
        : containFit(dims, AVAIL_MAIN, availCross);
      out.push({ item, index: i, offset: i * CELL, photoW: fit.w, photoH: fit.h });
    }
    return out;
  });

  function scrollMain(pos: number, behavior: ScrollBehavior = "auto") {
    strip?.scrollTo(vertical ? { top: pos, behavior } : { left: pos, behavior });
  }

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
    scrollPos = vertical ? strip.scrollTop : strip.scrollLeft;
    if (selfScroll) {
      selfScroll = false;
      return;
    }
    if (settings.lockCarousel && viewLen > 0 && items.length > 0 && !draggingSelf) {
      lockScrollTs = performance.now();
      const centered = Math.round((scrollPos + viewLen / 2 - CELL / 2) / CELL);
      const idx = Math.max(0, Math.min(items.length - 1, centered));
      if (idx !== session.focusedIndex) {
        session.focusedIndex = idx;
        session.selectionAnchor = idx;
      }
    }
  }
  // The strip div is destroyed/recreated every time the filmstrip is hidden
  // and shown again (the {#if} in the template swaps it for the peek button) and when it
  // changes side (the {#key}), so track its identity: a freshly (re)mounted
  // strip must always center instantly, never replaying a leftover "smooth" from
  // before, and starts from a zero scroll position.
  let lastStripEl: HTMLDivElement | null = null;
  // Derived, so another panel's drag does not rerun the effect on each pointer move.
  const ds = $derived(preview ? DOCK_IDLE : dock.state("filmstrip"));
  const draggingSelf = $derived(dock.drag?.panel === "filmstrip");
  const scrolledByUserJustNow = () =>
    settings.lockCarousel && performance.now() - lockScrollTs < 250;
  $effect(() => {
    // While the strip is dragged, the layout moves under it: recentre after the drop.
    if (preview || !strip || viewLen === 0 || draggingSelf) return;
    if (strip !== lastStripEl) {
      lastStripEl = strip;
      hasCentered = false;
      scrollPos = 0;
    }
    const focused = session.focusedIndex; // tracked for reactivity
    // In lock-carousel mode a focus change that came from the user's own strip
    // scroll must not trigger a counter-scroll (it would fight the drag). A focus
    // change from elsewhere (keyboard nav) still recentres, since no recent
    // scroll set lockScrollTs.
    if (scrolledByUserJustNow()) {
      hasCentered = true;
      return;
    }
    const target = Math.max(0, focused * CELL - viewLen / 2 + CELL / 2);
    const now = performance.now();
    const rapid = now - lastRecentre < RAPID_STEP_MS;
    lastRecentre = now;
    // Lock-carousel recentres INSTANTLY: a smooth scroll would fire onscroll on
    // intermediate frames whose centered cell reads as an earlier index, driving
    // the focus (and thus the loupe) briefly backwards — the "previous image
    // glitch" — before it lands. A jump has no intermediate frames.
    const instant = !hasCentered || rapid || settings.lockCarousel;
    selfScroll = true;
    scrollMain(target, instant ? "auto" : "smooth");
    // A recentre that changes nothing (already there, or clamped to the same
    // limit) fires no scroll event, so disarm on the next frame rather than
    // leaving the flag to swallow the user's next real scroll.
    requestAnimationFrame(() => (selfScroll = false));
    hasCentered = true;
  });

  // Drag the inner edge to resize; dragging it below COLLAPSE_AT hides the strip
  // (the peek arrow at the outer edge, and the F toggle, bring it back).
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

  // Distance from the pointer to the outer edge of the strip.
  function thicknessAt(rect: DOMRect, e: PointerEvent): number {
    switch (side) {
      case "bottom":
        return rect.bottom - e.clientY;
      case "top":
        return e.clientY - rect.top;
      case "left":
        return e.clientX - rect.left;
      case "right":
        return rect.right - e.clientX;
    }
  }

  function onResizeMove(e: PointerEvent) {
    if (!resizing || !strip) return;
    const t = thicknessAt(strip.getBoundingClientRect(), e);
    // Past the threshold only *flags* a pending collapse (strip stays at MIN_T,
    // dimmed) instead of committing it, so dragging back out cancels the hide.
    pendingCollapse = t < COLLAPSE_AT;
    const clamped = Math.min(contentMax(vertical), Math.max(MIN_T, t));
    if (vertical) session.filmstripWidth = clamped;
    else session.filmstripHeight = clamped;
  }

  function endResize(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    if (pendingCollapse) {
      pendingCollapse = false;
      session.setShowFilmstrip(false);
    } else if (vertical) {
      session.setFilmstripWidth(session.filmstripWidth);
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
    // scroll. We only read coordinates.
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
  {#if !preview}
    <DockGhost panel="filmstrip">
      {#snippet ghost(ghostSide, size)}
        <Filmstrip {items} {companionIndex} preview={{ side: ghostSide, size }} />
      {/snippet}
    </DockGhost>
  {/if}
  <!-- Keyed on the side: a side change remounts the strip, which then centres
       the focused cell at once instead of scrolling from a stale position. -->
  {#key side}
    <div
      class="filmstrip dock-panel {side}"
      class:vertical
      class:preview={!!preview}
      class:pending-collapse={pendingCollapse}
      class:docking={ds.docking}
      class:dock-moving={ds.moving}
      class:vacating={ds.vacating}
      bind:this={root}
      style="--thickness: {stripT}px"
    >
      {#if !preview}
      <div
        class="resize-handle"
        class:resizing
        role="separator"
        aria-orientation={vertical ? "vertical" : "horizontal"}
        aria-label="Resize filmstrip"
        onpointerdown={startResize}
        onpointermove={onResizeMove}
        onpointerup={endResize}
        onpointercancel={endResize}
      ></div>
      {/if}
      <!-- Hide tab on the inner edge: the grip, then the arrow towards the
           outer edge. Overlaid above the thumbnails (high z-index) instead of
           taking layout space — the same overlay pattern as the folder tree's
           hide tab. -->
      <div class="strip-tab">
        <DockGrip
          panel="filmstrip"
          still={!!preview}
          target={() => root?.parentElement ?? null}
          {thicknessFor}
          orientation={vertical ? "vertical" : "horizontal"}
        />
        {#if preview}
          <span class="strip-hide"><HideIcon size={16} /></span>
        {:else}
          <button
            class="strip-hide"
            title="Hide filmstrip"
            aria-label="Hide filmstrip"
            onclick={() => session.setShowFilmstrip(false)}
          >
            <HideIcon size={16} />
          </button>
        {/if}
      </div>
      <div
        class="strip"
        id={preview ? undefined : "filmstrip-scroll"}
        bind:this={strip}
        bind:clientWidth={clientW}
        bind:clientHeight={clientH}
        onscroll={preview ? undefined : onStripScroll}
      >
        <div
          class="canvas"
          style="{vertical ? 'height' : 'width'}:{items.length * CELL}px{preview
            ? `; transform: translate${vertical ? 'Y' : 'X'}(${-pos}px)`
            : ''}"
        >
          {#each visible as v (v.item.id)}
            {@const burst = session.burstPositionAt(v.index)}
            <div
              class="cell"
              class:focused={v.index === session.focusedIndex}
              class:companion={v.index !== session.focusedIndex && v.index === companionIndex}
              style="transform: translate{vertical ? 'Y' : 'X'}({v.offset}px); {vertical ? 'height' : 'width'}:{CELL}px"
              onpointerdown={preview ? undefined : (e) => onCellPointerDown(e, v.index)}
              onpointerup={preview ? undefined : onCellPointerUp}
              onpointercancel={preview ? undefined : resetTap}
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
                      alt=""
                      decoding="async"
                      draggable="false"
                      use:gridThumb={{
                        url: thumbUrl(v.item),
                        id: v.item.id,
                        exists: !!v.item.thumbReady,
                        retries: 0,
                        onfail: () => markPosterFailed(v.item.id),
                      }}
                    />
                  {/if}
                  <span class="videobadge"><Play size={9} /></span>
                {:else}
                  <!-- The grid's loader: the frame keeps a placeholder until its
                       thumbnail exists, never Chromium's broken-image glyph. -->
                  <img
                    alt=""
                    decoding="async"
                    draggable="false"
                    use:gridThumb={{ url: thumbUrl(v.item), id: v.item.id, exists: !!v.item.thumbReady }}
                  />
                {/if}
                {#if settings.filmstripShowLabel && v.item.label}
                  <span class="label-bar" style:border-color={LABEL_COLORS[v.item.label]}></span>
                {/if}
              </div>
              <div class="info-top">
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
                      title={pair ? pair.halves.map(describeHalf).join(" · ") : undefined}
                    >
                      <span class="type-name">
                        {#if v.item.decoupled}
                          <Scissors size={8} /><span>SPLIT</span>
                        {:else if pair?.diverged}
                          <span class="pair-halves">
                            {#each pair.halves as h, hi (h.id)}
                              {#if hi > 0}<span class="half-sep"></span>{/if}
                              <span class="half" class:struck={h.queuedDelete}>{h.name}</span>
                            {/each}
                          </span>
                        {:else}
                          <span class="type-full">RAW+JPG</span>
                          <span class="type-short">R+J</span>
                        {/if}
                      </span>
                    </span>
                  {:else if v.item.kind === 0}
                    <span class="chip">RAW</span>
                  {/if}
                {/if}
                {#if settings.filmstripShowFlag && v.item.flag !== 0}
                  <span class="flagbadge" class:pick={v.item.flag === 1} class:reject={v.item.flag === -1}>
                    {#if v.item.flag === 1}<Check size={11} />{:else}<X size={11} />{/if}
                  </span>
                {/if}
              </div>
              <div class="info-bottom">
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
      <!-- Overlay scrollbar floating over the outer edge of the cells, aligned
           with the strip's safe-area box. -->
      <div class="strip-overlay">
        <OverlayScrollbar
          orientation={vertical ? "vertical" : "horizontal"}
          edge={side === "top" || side === "left" ? "start" : "end"}
          viewport={viewLen}
          content={items.length * CELL}
          position={pos}
          controls={preview ? "" : "filmstrip-scroll"}
          onSeek={(to) => !preview && scrollMain(to)}
          alwaysVisible
        />
      </div>
    </div>
  {/key}
{:else}
  <button
    class="strip-peek {side}"
    title="Show filmstrip"
    aria-label="Show filmstrip"
    onclick={() => session.setShowFilmstrip(true)}
  >
    <PeekIcon size={16} />
  </button>
{/if}

<style>
  .filmstrip {
    position: relative;
    flex: none;
    height: var(--thickness);
    background: var(--surface);
  }

  .filmstrip.vertical {
    width: var(--thickness);
    height: auto;
  }

  /* A ghost fills the box that DockGhost gives it. */
  .filmstrip.preview {
    width: 100%;
    height: 100%;
  }

  .preview .strip {
    overflow: hidden;
  }

  /* The border is on the inner edge, towards the view. Top and left strips come
     first in the row or column of their container. */
  .filmstrip.bottom {
    border-top: 1px solid var(--border);
  }

  .filmstrip.top {
    order: -1;
    border-bottom: 1px solid var(--border);
  }

  .filmstrip.left {
    order: -1;
    border-right: 1px solid var(--border);
  }

  .filmstrip.right {
    border-left: 1px solid var(--border);
  }

  /* Edge-to-edge: the surface bleeds under a landscape navigation bar or
     cutout while the cells stay inside the safe area. */
  .filmstrip:not(.vertical),
  .filmstrip.left,
  .filmstrip.right {
    padding-left: var(--safe-left);
    padding-right: var(--safe-right);
  }

  .filmstrip.left {
    padding-right: 0;
  }

  .filmstrip.right {
    padding-left: 0;
  }

  /* Dragged past the collapse threshold: a subtle dim signals that releasing
     now will hide the strip (dragging back out cancels). */
  .filmstrip.pending-collapse {
    opacity: 0.6;
    transition: opacity 0.1s;
  }

  /* The strip is being dragged to another side. */
  .filmstrip.docking {
    opacity: 0.5;
  }

  /* On a drag, and on the way back after a cancel. Never on a drop, so the
     landed strip and the content gap swap in one frame. */
  .filmstrip.dock-moving {
    transition: height var(--dock-motion), width var(--dock-motion), opacity var(--dock-motion);
  }

  /* The preview is at another side: the strip closes its slot and the view takes
     the room. The node stays mounted, because the grip in it holds the pointer
     capture of the drag. Only the thickness shrinks, so the scroll axis keeps its
     length. */
  .filmstrip.docking.vacating {
    height: 0;
    opacity: 0;
    overflow: hidden;
    border-width: 0;
    padding: 0;
  }

  .filmstrip.docking.vacating.vertical {
    width: 0;
    height: auto;
  }

  .strip {
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
    /* Native bar hidden: a classic scrollbar would carve 8px out of the cell
       thickness. OverlayScrollbar floats over the thumbnails instead. */
    scrollbar-width: none;
  }

  .vertical .strip {
    overflow-x: hidden;
    overflow-y: auto;
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

  .vertical .strip-overlay {
    left: 0;
    right: 0;
  }

  .left .strip-overlay {
    left: var(--safe-left);
  }

  .right .strip-overlay {
    right: var(--safe-right);
  }

  /* Grabbable strip straddling the inner edge; a hairline reveals on hover/drag. */
  .resize-handle {
    position: absolute;
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
    height: 2px;
    background: transparent;
    transition: background 0.12s;
  }

  .bottom .resize-handle {
    top: -3px;
  }

  .top .resize-handle {
    bottom: -3px;
  }

  .bottom .resize-handle::after {
    top: 3px;
  }

  .top .resize-handle::after {
    bottom: 3px;
  }

  .vertical .resize-handle {
    top: 0;
    bottom: 0;
    left: auto;
    right: auto;
    width: 7px;
    height: auto;
    cursor: col-resize;
  }

  .vertical .resize-handle::after {
    top: 0;
    bottom: 0;
    left: 3px;
    right: auto;
    width: 2px;
    height: auto;
  }

  .left .resize-handle {
    right: -3px;
  }

  .right .resize-handle {
    left: -3px;
  }

  .resize-handle:hover::after,
  .resize-handle.resizing::after {
    background: var(--accent);
  }

  /* Peek tab shown at the outer edge when the filmstrip is collapsed. It floats
     (absolute) so the collapsed strip reserves NO space — the freed band goes
     back to the viewer/compare stage. Anchored to the view's outer edge (its
     parent .viewer/.compare is position:relative), centred along it. */
  .strip-peek {
    position: absolute;
    z-index: 10;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--accent);
    cursor: pointer;
  }

  .strip-peek.bottom,
  .strip-peek.top {
    left: 50%;
    transform: translateX(-50%);
  }

  .strip-peek.bottom {
    bottom: var(--safe-bottom);
    border-bottom: none;
    border-radius: var(--r-md) var(--r-md) 0 0;
  }

  .strip-peek.top {
    top: 0;
    border-top: none;
    border-radius: 0 0 var(--r-md) var(--r-md);
  }

  .strip-peek.left,
  .strip-peek.right {
    top: 50%;
    transform: translateY(-50%);
    width: 18px;
    height: 44px;
  }

  .strip-peek.left {
    left: var(--safe-left);
    border-left: none;
    border-radius: 0 var(--r-md) var(--r-md) 0;
  }

  .strip-peek.right {
    right: var(--safe-right);
    border-right: none;
    border-radius: var(--r-md) 0 0 var(--r-md);
  }

  .strip-peek:hover {
    background: var(--hover);
    border-color: var(--accent);
  }

  /* Hide tab on the inner edge, over the thumbnails: one piece, grip then arrow
     (the mirror of .strip-peek, pointing outward). It is 64 long: the arrow sits
     in the middle, with the grip's width as padding on the other side, so the
     arrow is where the peek tab's arrow is. Its z-index is above the resize
     handle so both parts stay clickable. */
  .strip-tab {
    position: absolute;
    z-index: 30;
    display: flex;
    box-sizing: border-box;
    width: 64px;
    height: 18px;
    overflow: hidden;
    border: 1px solid var(--border);
    background: var(--surface);
  }

  .strip-tab :global(button:focus-visible) {
    outline-offset: -2px;
  }

  .strip-tab:hover {
    background: var(--hover);
    border-color: var(--accent);
  }

  .bottom .strip-tab,
  .top .strip-tab {
    left: 50%;
    transform: translateX(-50%);
  }

  .bottom .strip-tab {
    top: 0;
    border-top: none;
    border-radius: 0 0 var(--r-md) var(--r-md);
  }

  .top .strip-tab {
    bottom: 0;
    border-bottom: none;
    border-radius: var(--r-md) var(--r-md) 0 0;
  }

  .vertical .strip-tab {
    flex-direction: column;
    top: 50%;
    transform: translateY(-50%);
    width: 18px;
    height: 64px;
  }

  .left .strip-tab {
    right: 0;
    border-right: none;
    border-radius: var(--r-md) 0 0 var(--r-md);
  }

  .right .strip-tab {
    left: 0;
    border-left: none;
    border-radius: 0 var(--r-md) var(--r-md) 0;
  }

  .filmstrip .strip-tab :global(.dock-grip) {
    width: 20px;
    height: auto;
    border-radius: 0;
    color: #8a8a93;
  }

  .filmstrip.vertical .strip-tab :global(.dock-grip) {
    width: auto;
    height: 20px;
  }

  .strip-hide {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 1;
    min-width: 0;
    padding: 0 20px 0 0;
    border: none;
    background: transparent;
    color: var(--accent);
    cursor: pointer;
  }

  .vertical .strip-hide {
    min-width: auto;
    min-height: 0;
    padding: 0 0 20px;
  }

  .canvas {
    position: relative;
    height: 100%;
  }

  .vertical .canvas {
    width: 100%;
    height: auto;
  }

  .cell {
    position: absolute;
    top: 0;
    height: 100%;
    /* Defines the box `visible`'s containFit() sizes .photo into (see AVAIL_MAIN/
       availCross in the script) — .photo ends up centered within whatever slack
       that leaves, via the flex centering below. */
    padding: 8px 3px;
    box-sizing: border-box;
    container-type: inline-size;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--r-md);
  }

  /* Transposed: the cell is a row of the column, so its padding swaps too. */
  .vertical .cell {
    left: 0;
    width: 100%;
    height: auto;
    padding: 3px 8px;
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
    border-radius: var(--r-sm);
  }

  img {
    width: 100%;
    height: 100%;
    object-fit: cover;
    user-select: none;
  }

  .photo.queued img {
    filter: brightness(0.4) grayscale(35%);
  }

  /* `data-thumb` is written by the `gridThumb` loader, as in the grid. */
  .photo:global([data-thumb="loading"]),
  .photo:global([data-thumb="failed"]) {
    background: var(--surface-2);
  }

  .photo:global([data-thumb="pending"]) {
    background: linear-gradient(100deg, var(--surface-2) 30%, var(--hover) 50%, var(--surface-2) 70%);
    background-size: 200% 100%;
    animation: frame-shimmer 2.4s ease-in-out infinite;
  }

  @keyframes frame-shimmer {
    from {
      background-position: 200% 0;
    }
    to {
      background-position: -200% 0;
    }
  }

  /* Android WebView draws its broken-image glyph for a sized <img> without a
     source; the placeholder behind it says the same thing better. */
  img:not([src]) {
    visibility: hidden;
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
    font-size: var(--fs-xs);
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
    font-size: var(--fs-xs);
    font-weight: 600;
    padding: 1px 4px;
    border-radius: var(--r-sm);
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

  /* Burst badge: the grid's pill, placed for a filmstrip cell. */
  .burst {
    position: absolute;
    top: 4px;
    left: 4px;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    font-size: var(--fs-xs);
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
    color: var(--danger);
  }

  /* Cell-width rows stay readable when a portrait photo is narrower than its badges. */
  .info-top,
  .info-bottom {
    position: absolute;
    left: 3px;
    right: 3px;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    pointer-events: none;
  }

  .info-top {
    top: 8px;
  }

  .info-bottom {
    bottom: 8px;
    justify-content: space-between;
  }

  /* Transposed with the cell padding, so the rows still sit on the photo. */
  .vertical .info-top,
  .vertical .info-bottom {
    left: 8px;
    right: 8px;
  }

  .vertical .info-top {
    top: 3px;
  }

  .vertical .info-bottom {
    bottom: 3px;
  }

  .info-top > .burst,
  .info-top > .chip,
  .info-top > .flagbadge,
  .info-bottom > .stars,
  .info-bottom > .tags {
    position: static;
    max-width: 100%;
    box-sizing: border-box;
  }

  .info-top > .chip {
    order: 2;
    overflow: hidden;
    white-space: nowrap;
  }

  .info-top > .chip .type-name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pair-halves {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .type-short {
    display: none;
  }

  /* The full RAW+JPG label and a flag need about 77 px of row, whatever the font.
     A horizontal cell has 90 px, so it always keeps the full label. */
  @container (max-width: 80px) {
    .type-full {
      display: none;
    }

    .type-short {
      display: inline;
    }
  }

  .info-top > .flagbadge {
    margin-left: auto;
    padding: 1px;
    border-radius: var(--r-sm);
    background: rgba(0, 0, 0, 0.55);
  }

  .info-bottom > .tags {
    flex-wrap: wrap;
  }

  .stars {
    position: absolute;
    bottom: 7px;
    left: 4px;
    color: #ffd166;
    font-size: var(--fs-xs);
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
