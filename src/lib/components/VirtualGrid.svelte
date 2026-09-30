<script module lang="ts">
  /**
   * Where the grid was left, kept across mounts. The component is destroyed
   * every time the view switches to the loupe/compare/survey, so the offset
   * cannot live in component state: returning would always land at the top.
   * Keyed by project, so opening another one starts from its own beginning.
   */
  let savedScroll: { root: string; top: number } | null = null;
</script>

<script lang="ts">
  import { tick } from "svelte";
  import { api, thumbUrl, displayDims, type ItemLite } from "../api";
  import { SvelteMap } from "svelte/reactivity";
  import { catalog } from "../stores/catalog.svelte";
  import { describeHalf, session } from "../stores/session.svelte";
  import { settings } from "../stores/settings.svelte";
  import { formatColorLabel } from "../colorLabels";
  import { tags } from "../stores/tags.svelte";
  import { view } from "../stores/view.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import { buildGridMenu } from "../gridMenu";
  import { IS_TOUCH } from "../platform";
  import type { MenuNode } from "../menu";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Play from "@lucide/svelte/icons/play";
  import Film from "@lucide/svelte/icons/film";
  import FileWarning from "@lucide/svelte/icons/file-warning";
  import Loader from "@lucide/svelte/icons/loader";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Layers from "@lucide/svelte/icons/layers";
  import { edgeBounce } from "../anim";
  import { bucketOf } from "../gridGroups";

  let { items }: { items: ItemLite[] } = $props();

  // The grid draws CELLS, not items: with bursts collapsed one cell can stand
  // for a whole run of consecutive frames. `session.gridCellStarts` is null
  // when the two coincide, and these helpers hide the difference from the
  // layout, hit-testing and marquee code below. A cell index addresses the
  // grid; `session` speaks `filtered` indexes, so every call into it goes
  // through `cellFirst`.
  const cellStarts = $derived(session.gridCellStarts);
  const cellCount = $derived(cellStarts ? cellStarts.length : items.length);

  function cellFirst(cell: number): number {
    return cellStarts ? cellStarts[cell] : cell;
  }

  function cellSpan(cell: number): number {
    if (!cellStarts) return 1;
    return (cell + 1 < cellStarts.length ? cellStarts[cell + 1] : items.length) - cellStarts[cell];
  }

  function cellItem(cell: number): ItemLite {
    return items[cellFirst(cell)];
  }

  /** What a collapsed burst has been culled to, summed over the frames it hides.
   *  `rejects` counts a frame already on its way out by either route (the reject
   *  flag or a queued delete), the same pair the dimming uses. */
  function stackSummary(first: number, span: number) {
    let picks = 0;
    let rejects = 0;
    let maxRating = 0;
    for (let i = first; i < first + span; i++) {
      const it = items[i];
      if (!it) continue;
      if (it.flag === 1) picks++;
      else if (it.flag === -1 || session.pendingDeleteIds.has(it.id)) rejects++;
      if (it.rating > maxRating) maxRating = it.rating;
    }
    return { picks, rejects, maxRating };
  }

  function stackTitle(sum: ReturnType<typeof stackSummary>, span: number): string {
    return `${sum.picks} picked, ${sum.rejects} rejected of ${span}`;
  }

  const OVERSCAN_ROWS = 2;
  const LONG_PRESS_MS = 400;
  const MARGIN_Y = 14;
  const MARGIN_TOP = 6;
  // Inter-cell gap: the pitch (CELL) still tiles edge-to-edge for all hit-test
  // and marquee math, but each cell's visual box is inset by GAP so adjacent
  // focus/selection outlines never touch. Kept out of the pitch math on purpose.
  const GAP = 6;
  // Carried by whichever cell is focused, so aria-activedescendant always
  // resolves. Naming it after the item id would break on a collapsed burst,
  // whose focused member is not the cell the grid draws.
  const ACTIVE_CELL_ID = "grid-active-cell";
  // Selected cells shrink a touch further — extra breathing room so a block of
  // adjacent selections never reads as one solid blue mass. Computed as a
  // plain extra inset (added to the existing GAP/2 offset, subtracted twice
  // from the width/height) rather than a CSS `scale`: composing `scale` with
  // the cell's own `transform: translate(...)` positioning read as selected
  // cells overlapping their neighbors instead of shrinking cleanly in place.
  const SELECTED_INSET = 5;

  // A small pill (bottom-right, over the cells) reporting whatever background
  // work is in flight: scanning, then metadata, then the artifact passes. All
  // status lives here — the top bar never shows these messages.
  const bgStatus = $derived.by(() => {
    if (catalog.scanning) return `Scanning… ${catalog.scanFound || 0}`;
    // Highest priority after the scan: until this finishes the grid is ordered
    // by file date rather than capture time, so say so rather than leaving the
    // reorder unexplained.
    const m = catalog.metaProgress;
    if (m.total > 0) return `Reading photo info ${m.done} / ${m.total}`;
    // Neither of these two is "the thumbnail phase". The first is a fixed
    // start-up window (LEAD_WINDOW) that fills the top of the grid; the second
    // is the fused pass, which writes the thumbnail *and* the preview for
    // everything else. Naming the first one "Thumbnails N / 60" reported a
    // constant as a denominator and made a fresh import of hundreds of photos
    // look nearly finished at 60.
    const t = catalog.thumbProgress;
    if (t.total > 0) return `Preparing first photos ${t.done} / ${t.total}`;
    const p = catalog.previewProgress;
    if (p.total > 0) return `Preparing photos ${p.done} / ${p.total}`;
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

  // A thumbnail request the backend refused -- almost always because the pool
  // evicted it to keep a fast scroll from banking minutes of decoding. The cell
  // is still on screen, so ask again rather than leaving a skeleton there
  // forever. Capped, so a genuinely broken file cannot spin.
  const MAX_THUMB_RETRIES = 3;
  const RETRY_DELAY_MS = 400;
  const thumbRetry = new SvelteMap<number, number>();

  function retryThumb(id: number) {
    const n = thumbRetry.get(id) ?? 0;
    if (n >= MAX_THUMB_RETRIES) return;
    setTimeout(() => thumbRetry.set(id, n + 1), RETRY_DELAY_MS);
  }

  /** Thumbnail URL, re-minted on each retry so the browser refetches it. */
  function thumbSrc(item: ItemLite): string {
    const n = thumbRetry.get(item.id);
    return n ? `${thumbUrl(item)}&retry=${n}` : thumbUrl(item);
  }

  // Video ids whose pregenerated poster couldn't be served (no frame extractor
  // on this platform, or the clip was undecodable → 404). Those cells fall back
  // to a live <video>.
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
  // Pull-to-refresh begins at the top and owns that position until its visual
  // cycle settles. Catalog refreshes rebuild `layout`; without this guard the
  // focus-following effect can mistake that rebuild for navigation and jump to
  // an old, off-screen focus.
  let pullRefreshOwnsTop = false;

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

  // Cell pitch (thumbnail + label + gap), scaled by the density preference.
  // Narrower base on phones so several columns fit; density then nudges it.
  const DENSITY: Record<string, number> = { small: 0.72, medium: 1, large: 1.34 };
  const isNarrow = $derived(width > 0 && width < 520);
  const BASE_CELL = $derived((isNarrow ? 116 : 188) * (DENSITY[session.gridDensity] ?? 1));
  const isTouch = IS_TOUCH;
  // Never collapse below two columns: on very narrow viewports keep 2 columns
  // and shrink the cells to fit instead.
  const MIN_COLS = 2;
  const cols = $derived(width > 0 ? Math.max(MIN_COLS, Math.floor(width / BASE_CELL)) : 1);
  // Touch, and any narrow viewport: the row is justified — all but a hair of side
  // margin is given back to the cells, and the base pitch's leftover is spread
  // across the columns so they tile the width EXACTLY at every thumbnail size.
  // Keying that on width alone left every touch screen wider than 520px (tablet,
  // foldable, phone in landscape) on the desktop path, where a bigger thumbnail
  // size means a bigger leftover and so wider dead margins. Desktop keeps the
  // base pitch and centres the block, splitting the leftover as equal margins.
  const fillWidth = $derived(isTouch || isNarrow);
  const EDGE = $derived(fillWidth ? 2 : 0);
  const CELL = $derived(
    fillWidth
      ? Math.max(1, (width - EDGE * 2) / cols)
      : cols * BASE_CELL <= width
        ? BASE_CELL
        : Math.max(1, Math.floor(width / cols)),
  );
  const padX = $derived(EDGE + Math.max(0, (width - EDGE * 2 - cols * CELL) / 2));

  /** Whether cells may animate their move. A geometry change moves every cell
   *  at once, which reads as the grid shuffling itself: opening a project runs
   *  `cols` from 1 to N the moment the viewport reports its width, and a window
   *  resize does the same. The transition is there for the selection shrink, so
   *  suppress it while the layout itself is what changed and restore it once
   *  the new geometry has painted. */
  let animateCells = $state(false);
  $effect(() => {
    void cols;
    void CELL;
    animateCells = false;
    // Two frames: one for the new geometry to be applied, one for it to paint.
    // Re-enabling any earlier animates the very move being suppressed.
    let inner = 0;
    const outer = requestAnimationFrame(() => {
      inner = requestAnimationFrame(() => (animateCells = true));
    });
    return () => {
      cancelAnimationFrame(outer);
      if (inner) cancelAnimationFrame(inner);
    };
  });

  // Report layout so keyboard ↑/↓ move one visual row.
  $effect(() => {
    session.gridCols = cols;
  });

  // Section-header heights: taller for the outermost grouping level.
  const HEADER_H0 = 34;
  const HEADER_H1 = 27;
  function headerH(depth: number): number {
    return depth === 0 ? HEADER_H0 : HEADER_H1;
  }

  type Header = {
    depth: number;
    label: string;
    count: number;
    y: number;
    h: number;
    /** Stable identity for this section (the joined bucket path down to this
     *  depth) — the collapsed-set key and the DOM #each key. */
    key: string;
    /** Cell range this section covers ([start, end)), including anything hidden
     *  under a collapsed sub-section — used for the group select-all checkbox. */
    start: number;
    end: number;
    collapsed: boolean;
  };

  // Section keys the user has collapsed (component-local — a fresh grid mount,
  // e.g. reopening a project, starts fully expanded). Keyed by Header.key, so
  // it survives re-sorts/filters as long as the same bucket path still exists.
  let collapsedKeys = $state<Set<string>>(new Set());

  function toggleCollapsed(key: string) {
    const next = new Set(collapsedKeys);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsedKeys = next;
  }

  /** The `items` range a header's cell range covers. */
  function headerItems(h: Header): { from: number; to: number } {
    return {
      from: cellFirst(h.start),
      to: h.end >= cellCount ? items.length : cellFirst(h.end),
    };
  }

  /** Whether every item in a header's section is currently selected (the
   *  checkbox's checked state) — false for an empty range. */
  function groupAllSelected(h: Header): boolean {
    if (h.end <= h.start) return false;
    const { from, to } = headerItems(h);
    for (let i = from; i < to; i++) {
      if (!session.selectedIds.has(items[i].id)) return false;
    }
    return true;
  }

  /** Select (or, if already fully selected, deselect) every item in a header's
   *  section. Adds to/subtracts from the existing selection rather than
   *  replacing it, so group checkboxes compose across sections. */
  function toggleGroupSelect(h: Header) {
    const selectAll = !groupAllSelected(h);
    const next = new Set(session.selectedIds);
    const { from, to } = headerItems(h);
    for (let i = from; i < to; i++) {
      const id = items[i].id;
      if (selectAll) next.add(id);
      else next.delete(id);
    }
    session.selectedIds = next;
    // No focus while a selection exists (matches toggleSelect/rangeSelect).
    session.focusedIndex = -1;
  }

  // One pass over `items` places every cell and injects a section header wherever
  // an active grouping level's bucket changes. With no grouping active this
  // degenerates to the old uniform grid (no headers; cells flow row by row), so
  // the default path is behaviourally unchanged. `itemY` comes out non-decreasing,
  // which lets the visible-window and hit-testing code binary-search it.
  const layout = $derived.by(() => {
    const group = session.groupBy;
    const n = cellCount;
    const itemX = new Float64Array(n);
    const itemY = new Float64Array(n);
    const hidden = new Uint8Array(n);
    const headers: Header[] = [];

    // Per-cell bucket paths + prefix counts (for the header "· N" tallies). A
    // collapsed burst is bucketed by its first frame and counts once, matching
    // what the section actually draws.
    const paths: string[][] = [];
    const labels: string[][] = [];
    const counts = new Map<string, number>();
    if (group.length > 0) {
      for (let i = 0; i < n; i++) {
        const keys: string[] = [];
        const labs: string[] = [];
        let prefix = "";
        for (let L = 0; L < group.length; L++) {
          const b = bucketOf(cellItem(i), group[L], session.groupContext);
          keys.push(b?.key ?? "~");
          labs.push(b?.label ?? "—");
          prefix += `\u0000${keys[L]}`;
          counts.set(prefix, (counts.get(prefix) ?? 0) + 1);
        }
        paths.push(keys);
        labels.push(labs);
      }
    }

    let y = MARGIN_TOP;
    let col = 0;
    let prev: string[] | null = null;
    // Headers currently open at each depth, so a later boundary can close them
    // (set their `end`) without a second pass over `items`.
    const openStack: (Header | null)[] = new Array(group.length).fill(null);
    // Depth of the shallowest collapsed ancestor currently in effect, or -1.
    // While set, neither sub-headers nor real cell positions are created.
    let collapsedDepth = -1;
    for (let i = 0; i < n; i++) {
      if (group.length > 0) {
        // First level whose bucket differs from the previous item → a boundary;
        // that level and every deeper one start a fresh section.
        let boundary = -1;
        if (prev === null) boundary = 0;
        else {
          let d = 0;
          while (d < group.length && paths[i][d] === prev[d]) d++;
          if (d < group.length) boundary = d;
        }
        if (boundary >= 0) {
          for (let d = group.length - 1; d >= boundary; d--) {
            if (openStack[d]) {
              openStack[d]!.end = i;
              openStack[d] = null;
            }
          }
          if (collapsedDepth >= boundary) collapsedDepth = -1;
          if (collapsedDepth === -1) {
            if (col > 0) {
              y += CELL;
              col = 0;
            }
            let acc = "";
            for (let L = 0; L < group.length; L++) {
              acc += `\u0000${paths[i][L]}`;
              if (L < boundary) continue;
              const key = acc;
              const isCollapsed = collapsedKeys.has(key);
              const hdr: Header = {
                depth: L,
                label: labels[i][L],
                count: counts.get(acc) ?? 0,
                y,
                h: headerH(L),
                key,
                start: i,
                end: n,
                collapsed: isCollapsed,
              };
              headers.push(hdr);
              openStack[L] = hdr;
              y += headerH(L);
              if (isCollapsed) {
                collapsedDepth = L;
                break; // deeper sub-headers/items of a collapsed section aren't laid out
              }
            }
          }
        }
        prev = paths[i];
      }

      if (group.length > 0 && collapsedDepth !== -1) {
        // Hidden under a collapsed ancestor: park at the header's y, exclude
        // from rendering/hit-testing (see `visible`/`applyMarquee`/`hitTest`).
        itemY[i] = y;
        itemX[i] = NaN;
        hidden[i] = 1;
        continue;
      }

      itemX[i] = padX + col * CELL;
      itemY[i] = y;
      col++;
      if (col >= cols) {
        col = 0;
        y += CELL;
      }
    }
    if (col > 0) y += CELL;
    for (const hdr of openStack) if (hdr) hdr.end = n;
    return { itemX, itemY, hidden, headers, contentHeight: y + MARGIN_Y };
  });

  const contentHeight = $derived(layout.contentHeight);

  // First array index whose value is >= target (ascending array).
  function lowerBound(arr: Float64Array, target: number): number {
    let lo = 0;
    let hi = arr.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (arr[mid] < target) lo = mid + 1;
      else hi = mid;
    }
    return lo;
  }

  /** Scroll the focused cell into view, moving as little as possible. Uses the
   *  laid-out y, so it stays correct with headers between groups. */
  function followFocus() {
    if (!viewport || session.focusedIndex === -1) return;
    const top = layout.itemY[session.gridCellAt(session.focusedIndex)];
    if (top === undefined) return;
    const bottom = top + CELL;
    if (top < viewport.scrollTop) viewport.scrollTo({ top });
    else if (bottom > viewport.scrollTop + height) viewport.scrollTo({ top: bottom - height });
  }

  function keepRefreshAtTop() {
    if (!viewport) return;
    viewport.scrollTop = 0;
    scrollTop = 0;
    savedScroll = { root: catalog.project?.rootPath ?? "", top: 0 };
  }

  // True once the offset saved by the previous visit has been re-applied. Until
  // then `followFocus` must not run: it would scroll from the top of the grid
  // and win over the restore that is about to happen.
  let restored = false;

  // Come back to the grid where it was left. The offset alone is not enough:
  // the loupe can page far away from what the grid last showed, so the saved
  // position only stands if the focused photo is still on screen under it —
  // otherwise the focused cell is centred instead.
  //
  // Deliberately awaits `tick()`. This effect is created before the template's,
  // so on the flush that first reports the viewport size the canvas still
  // carries the height computed from the pre-measurement layout (a few hundred
  // px). Scrolling then would be clamped to that stale height — the grid landed
  // near its top no matter which photo was open.
  async function restoreScroll() {
    if (!viewport) return;
    await tick();
    if (!viewport) return;
    const saved =
      savedScroll && savedScroll.root === catalog.project?.rootPath ? savedScroll.top : 0;
    viewport.scrollTop = saved;
    const top =
      session.focusedIndex === -1
        ? undefined
        : layout.itemY[session.gridCellAt(session.focusedIndex)];
    if (top !== undefined && (top < saved || top + CELL > saved + height)) {
      viewport.scrollTop = Math.max(0, top - (height - CELL) / 2);
    }
    scrollTop = viewport.scrollTop;
  }

  // Keep the focused cell in view on keyboard navigation. Skipped when nothing
  // is focused (-1) — a selection in progress deliberately drops focus (see
  // beginMarquee) — and until `height`/`width` report a real measurement (a
  // tick after mount), which is also when the restore above can run.
  $effect(() => {
    void session.focusedIndex;
    void layout;
    if (!viewport || width === 0 || height === 0) return;
    if (!restored) {
      restored = true;
      void restoreScroll();
      return;
    }
    if (pullRefreshOwnsTop) {
      keepRefreshAtTop();
      return;
    }
    followFocus();
  });

  // Only the visible window is materialized. The #each below is keyed by file
  // id so each thumbnail owns a stable <img>: scrolling adds/removes cells
  // instead of reassigning `src` on reused nodes.
  const visible = $derived.by(() => {
    const { itemX, itemY, hidden } = layout;
    const n = cellCount;
    const top = scrollTop - OVERSCAN_ROWS * CELL;
    const bot = scrollTop + height + OVERSCAN_ROWS * CELL;
    const start = lowerBound(itemY, top - CELL); // itemY >= top-CELL ⇒ cell bottom >= top
    const out: {
      item: ItemLite;
      index: number;
      /** First `items` index this cell draws. */
      first: number;
      /** Frames stacked under it (1 for a plain photo). */
      span: number;
      x: number;
      y: number;
    }[] = [];
    for (let i = start; i < n; i++) {
      if (itemY[i] > bot) break;
      if (hidden[i]) continue; // parked under a collapsed section — not rendered
      out.push({
        item: cellItem(i),
        index: i,
        first: cellFirst(i),
        span: cellSpan(i),
        x: itemX[i],
        y: itemY[i],
      });
    }
    return out;
  });

  const visibleHeaders = $derived.by(() =>
    layout.headers.filter((h) => h.y + h.h >= scrollTop - HEADER_H0 && h.y <= scrollTop + height),
  );

  // Group path whose section currently sits under the top of the viewport,
  // pinned as an overlay when the sticky option is on. Headers ascend by y, so
  // the last header at each depth at or above scrollTop is the current path.
  const stickyHeader = $derived.by(() => {
    if (!session.stickyGroupHeader || session.groupBy.length === 0) return null;
    const path: Header[] = [];
    for (const h of layout.headers) {
      if (h.y > scrollTop) break;
      // Replacing an ancestor invalidates every descendant from the previous
      // branch. Headers at the new boundary then refill the path in order.
      path.length = h.depth;
      path.push(h);
    }
    if (path.length === 0) return null;
    return {
      label: path.map((h) => h.label).join(" / "),
      count: path[path.length - 1].count,
    };
  });

  const KINETIC_SCROLL_MIN_SPEED = 0.6; // px/ms
  const KINETIC_SCROLL_GUARD_MS = 180;
  const TAP_SCROLL_SLOP = 2;
  let lastScrollSampleAt = 0;
  let lastScrollSampleTop = 0;
  let suppressTouchTapUntil = 0;

  function onScroll() {
    if (!viewport) return;
    const now = performance.now();
    const nextTop = viewport.scrollTop;
    const dt = now - lastScrollSampleAt;
    const distance = Math.abs(nextTop - lastScrollSampleTop);
    if (
      !pullRefreshOwnsTop &&
      lastScrollSampleAt > 0 &&
      dt > 0 &&
      distance / dt >= KINETIC_SCROLL_MIN_SPEED
    ) {
      suppressTouchTapUntil = now + KINETIC_SCROLL_GUARD_MS;
    }
    lastScrollSampleAt = now;
    lastScrollSampleTop = nextTop;
    // Momentum can move the viewport without a pointermove for the new contact
    // used to stop it. That displacement still makes the contact a scroll.
    if (touchTap && Math.abs(nextTop - touchTap.scrollTop) > TAP_SCROLL_SLOP) {
      touchTap.moved = true;
      cancelLongPress();
    }
    scrollTop = nextTop;
    // Recorded as it changes rather than on unmount, so the last position is
    // already stored whatever order the teardown runs in.
    if (restored) savedScroll = { root: catalog.project?.rootPath ?? "", top: scrollTop };
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
  let touchTap: {
    index: number;
    onCell: boolean;
    x: number;
    y: number;
    scrollTop: number;
    moved: boolean;
    suppressForKineticScroll: boolean;
  } | null = null;
  // Touch: last tap on a cell while a selection was active, for double-tap-to-open.
  const DOUBLE_TAP_MS = 320;
  let lastSelTap: { index: number; time: number } | null = null;

  function cancelLongPress() {
    if (longPressTimer) {
      clearTimeout(longPressTimer);
      longPressTimer = null;
    }
  }

  // Destroy mid-drag would otherwise leave a self-rescheduling edge-scroll frame
  // and a long-press timer that fires on a detached element. Cancel both (and
  // drop the drag) on unmount.
  $effect(() => () => {
    if (edgeRaf) {
      cancelAnimationFrame(edgeRaf);
      edgeRaf = 0;
    }
    cancelLongPress();
    drag = null;
  });

  /** Geometry hit-test against the laid-out cell positions (which may include
   *  group headers). Returns null off-grid (scrollbar), else the hit cell —
   *  `onCell` false when the point falls in a header, a gutter, or past the end. */
  function hitTest(e: { clientX: number; clientY: number }): { x: number; y: number; index: number; onCell: boolean } | null {
    if (!viewport) return null;
    const rect = viewport.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const viewY = e.clientY - rect.top;
    if (x >= viewport.clientWidth) return null; // scrollbar, not the grid
    const y = viewY + viewport.scrollTop;
    const { itemX, itemY, hidden } = layout;
    const n = cellCount;
    if (n === 0) return { x, y, index: 0, onCell: false };
    // Greatest index with itemY <= y: the row at or above the point.
    const k = lowerBound(itemY, y + 1e-6) - 1;
    if (k < 0) return { x, y, index: 0, onCell: false };
    // A collapsed section's parked items share the header's y — landing there
    // is a dead zone (visually the header/blank space), never a real cell.
    if (hidden[k] || y >= itemY[k] + CELL) return { x, y, index: k, onCell: false }; // header / gutter
    const rowStart = k - Math.round((itemX[k] - padX) / CELL);
    const targetCol = Math.floor((x - padX) / CELL);
    if (targetCol < 0 || targetCol >= cols) return { x, y, index: k, onCell: false };
    const idx = rowStart + targetCol;
    const onCell = idx >= 0 && idx < n && itemY[idx] === itemY[k];
    return { x, y, index: onCell ? idx : k, onCell };
  }

  // Last pointer type seen, so onDblClick (a native MouseEvent) can tell touch
  // from mouse/pen — see its own comment for why that matters.
  let lastPointerType = "mouse";

  /** Open menu: its anchor point and the tree to draw. */
  let menu = $state<{ x: number; y: number; items: MenuNode[] } | null>(null);

  /** Right-click. Windows-style targeting: inside the selection the menu acts on
   *  the whole selection, outside it the click first replaces the selection with
   *  the cell it landed on — so what the menu will hit is always what is drawn
   *  as selected. Built AFTER that, because every command reads
   *  `session.targets()`. */
  function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    // A menu opening mid-marquee would act on a selection still being dragged.
    cancelLongPress();
    drag = null;
    marquee = null;

    const hit = hitTest(e);
    let index: number | null = null;
    if (hit?.onCell) {
      index = cellFirst(hit.index);
      const inSelection = session.selectedIds.has(items[index].id);
      if (!inSelection) session.selectOnly(index);
      else index = session.focusedIndex >= 0 ? session.focusedIndex : index;
    } else {
      // Empty space inside the grid targets nothing, so nothing stays targeted —
      // the same reading a left click there already has, and it matches the
      // view-wide menu about to open.
      session.clearSelection();
      session.focusedIndex = -1;
    }
    menu = { x: e.clientX, y: e.clientY, items: buildGridMenu(index) };
  }

  function onPointerDown(e: PointerEvent) {
    lastPointerType = e.pointerType;
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
      touchTap = {
        index,
        onCell,
        x: e.clientX,
        y: e.clientY,
        scrollTop: viewport.scrollTop,
        moved: false,
        suppressForKineticScroll: performance.now() < suppressTouchTapUntil,
      };
      longPressTimer = setTimeout(() => {
        longPressTimer = null;
        touchTap = null; // became a marquee, not a tap
        beginMarquee(true);
      }, LONG_PRESS_MS);
      return;
    }

    // Mouse/pen: select immediately, then arm the marquee.
    if (onCell) {
      if (e.shiftKey) session.rangeSelect(cellFirst(index), e.ctrlKey);
      else if (e.ctrlKey) session.toggleSelect(cellFirst(index));
      else session.selectOnly(cellFirst(index));
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

    // Rectangle-intersect the laid-out cell boxes. itemY ascends, so start at the
    // first cell whose bottom reaches y0 and stop once a cell's top passes y1.
    // Touching a collapsed burst takes every frame stacked under it.
    const { itemX, itemY } = layout;
    const n = cellCount;
    const next = new Set(drag.base);
    for (let i = lowerBound(itemY, y0 - CELL); i < n; i++) {
      if (itemY[i] > y1) break;
      const ix = itemX[i];
      if (ix + CELL > x0 && ix < x1) {
        const from = cellFirst(i);
        for (let k = from; k < from + cellSpan(i); k++) next.add(items[k].id);
      }
    }
    session.selectedIds = next;
  }

  /** Double-click a cell to open the loupe. Pointer capture (set in
   *  onPointerDown) redirects click/dblclick hit-testing to .viewport, so
   *  this can't live on the .cell element — recompute the hit cell instead. */
  function onDblClick(e: MouseEvent) {
    // Without preventDefault() anywhere in the touch handling, the browser/
    // WebView still synthesizes compatibility mouse events (click, dblclick)
    // from touch gestures — including from two ordinary, separate taps that
    // land close together in time and space (e.g. quickly tapping two adjacent
    // cells while building a selection). That synthetic dblclick would open the
    // loupe unconditionally here, bypassing the selection-aware double-tap
    // logic in endDrag (which only opens on a genuine double-tap of the SAME
    // cell). Touch already has its own, more precise double-tap-to-open path,
    // so ignore the native dblclick whenever the last pointer was a touch.
    if (lastPointerType === "touch") return;
    const hit = hitTest(e);
    if (!hit?.onCell) return;
    // Focus the double-clicked photo before opening so the loupe shows IT. With a
    // selection active, focus is intentionally dropped (a selection has no single
    // "focused" cell), so without this the loupe would open on nothing. A
    // collapsed burst opens on its first frame, where , and . step the rest.
    session.selectOnly(cellFirst(hit.index));
    view.markOpenedFromGrid();
    view.mode = "viewer";
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
      const shiftedWhileDown =
        viewport !== null && Math.abs(viewport.scrollTop - tap.scrollTop) > TAP_SCROLL_SLOP;
      if (
        e.type !== "pointercancel" &&
        !tap.moved &&
        !tap.suppressForKineticScroll &&
        !shiftedWhileDown
      ) {
        if (!tap.onCell) {
          // A clean tap on empty grid space clears the whole selection, matching
          // the desktop empty-space click (which resolves to an empty marquee).
          session.clearSelection();
        } else if (session.selectedIds.size > 0) {
          // A selection is already active (started via long-press): a single tap
          // toggles membership so you can build a multi-selection one tap at a
          // time — it must NOT open (focus is dropped while selecting, so opening
          // would land the loupe on nothing). A quick double-tap on the same cell
          // opens it instead, focusing it first.
          const now = performance.now();
          if (
            lastSelTap &&
            lastSelTap.index === tap.index &&
            now - lastSelTap.time < DOUBLE_TAP_MS
          ) {
            lastSelTap = null;
            session.selectOnly(cellFirst(tap.index));
            view.markOpenedFromGrid();
            view.mode = "viewer";
          } else {
            lastSelTap = { index: tap.index, time: now };
            session.toggleSelect(cellFirst(tap.index));
          }
        } else {
          session.selectOnly(cellFirst(tap.index));
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

  // --- touch pull-to-refresh (mobile) ---
  // Dragging down while already scrolled to the top reveals a spinner and, past
  // a threshold, rescans the project folder — the same social-app gesture, so
  // phones don't need the toolbar's rescan button (moved to the title bar on
  // desktop). Touch-only: these listeners never fire on a mouse, so desktop is
  // unaffected. Runs in parallel with the pointer-event selection machinery — a
  // downward drag trips its TAP_SLOP/long-press cancellation, so it never also
  // starts a marquee or a tap.
  const PULL_MAX = 120; // hard cap on how far the content can be dragged
  const PULL_THRESHOLD = 48; // release past this to trigger a rescan
  const PULL_REST = 44; // spinner's resting offset while refreshing
  const PULL_START_SLOP = 8; // ignore jitter this small so a tap never engages a pull
  const MIN_SPIN_MS = 1000; // keep the spinner up at least this long

  let pullY = $state(0);
  let pullDragging = $state(false); // finger down and owning the gesture (no transition)
  let refreshing = $state(false);
  let pullStartY = 0;
  let pulling = false; // gesture candidate: began at the very top

  // Elastic resistance: fast at first, asymptotic toward PULL_MAX so it can't be
  // dragged arbitrarily far and feels rubber-banded.
  function resist(dy: number): number {
    return PULL_MAX * (1 - Math.exp(-dy / PULL_MAX));
  }

  // Only an ACTIVE marquee suppresses pull-to-refresh: a long-press that already
  // fired into a selection (`drag.active`), or a live marquee. That is the
  // "held the finger still long enough to start selecting, now dragging onto the
  // cells below" case that must win over a pull.
  //
  // A merely PENDING `longPressTimer` must NOT count here. It is armed on every
  // touch — including the first frames of an ordinary pull-down — and the pull
  // engages (8px) before the timer is cancelled (>10px of movement), so testing
  // it aborted the pull on its very first move and `pulling` never recovered.
  // A genuine pull cancels the pending long-press itself (onPointerMove past
  // TAP_SLOP), so it can never turn into a marquee; a genuine selection only
  // reaches "dragging onto the cells below" after the long-press has fired, when
  // `drag.active` is already true. So `drag.active` is the correct, sufficient
  // signal, and the pending-timer term merely broke the pull entirely.
  function marqueeEngaged(): boolean {
    return (drag?.active ?? false) || marquee !== null;
  }

  function onTouchStart(e: TouchEvent) {
    if (
      refreshing ||
      marqueeEngaged() ||
      e.touches.length !== 1 ||
      !viewport ||
      viewport.scrollTop > 0
    ) {
      pulling = false;
      return;
    }
    pulling = true;
    pullStartY = e.touches[0].clientY;
  }

  function onTouchMove(e: TouchEvent) {
    if (!pulling || refreshing || !viewport) return;
    // A marquee took over mid-gesture (the long-press fired into a selection):
    // abandon the pull and hand the drag to the selection machinery.
    if (marqueeEngaged()) {
      pulling = false;
      pullDragging = false;
      pullY = 0;
      return;
    }
    const dy = e.touches[0].clientY - pullStartY;
    // A non-downward move, or the list having scrolled, ends the pull and hands
    // the gesture back to native scrolling.
    if (dy <= 0 || viewport.scrollTop > 0) {
      pulling = false;
      pullDragging = false;
      pullY = 0;
      return;
    }
    if (dy < PULL_START_SLOP) return; // jitter / start of a tap — don't engage yet
    e.preventDefault(); // own the gesture: no native overscroll while pulling
    pullDragging = true;
    pullY = resist(dy - PULL_START_SLOP);
  }

  function onTouchEnd() {
    if (!pulling) return;
    pulling = false;
    pullDragging = false;
    if (pullY >= PULL_THRESHOLD) void doRefresh();
    else pullY = 0;
  }

  async function doRefresh() {
    refreshing = true;
    pullRefreshOwnsTop = true;
    keepRefreshAtTop();
    pullY = PULL_REST;
    const started = Date.now();
    // The rescan is fire-and-forget (its scan:* events drive the catalog); hold
    // the spinner a beat so an instant, no-op rescan still reads as a deliberate
    // refresh rather than a flicker.
    const wait = Math.max(0, MIN_SPIN_MS - (Date.now() - started));
    await Promise.allSettled([
      api.rescanProject(),
      new Promise((resolve) => setTimeout(resolve, wait)),
    ]);
    refreshing = false;
    pullY = 0;
  }

  // A large project can keep scanning after the pull animation has returned.
  // Release ownership only after both have ended; otherwise scan:done rebuilds
  // the layout later and focus-following can still jump to the old focus.
  $effect(() => {
    if (!pullRefreshOwnsTop || refreshing || catalog.scanning) return;
    let cancelled = false;
    void tick().then(() => {
      if (cancelled || refreshing || catalog.scanning) return;
      keepRefreshAtTop();
      pullRefreshOwnsTop = false;
    });
    return () => {
      cancelled = true;
    };
  });

  // Attach the touch listeners manually: touchmove must be non-passive so its
  // preventDefault (suppressing native overscroll) actually takes effect.
  $effect(() => {
    const el = viewport;
    if (!el) return;
    el.addEventListener("touchstart", onTouchStart, { passive: true });
    el.addEventListener("touchmove", onTouchMove, { passive: false });
    el.addEventListener("touchend", onTouchEnd, { passive: true });
    el.addEventListener("touchcancel", onTouchEnd, { passive: true });
    return () => {
      el.removeEventListener("touchstart", onTouchStart);
      el.removeEventListener("touchmove", onTouchMove);
      el.removeEventListener("touchend", onTouchEnd);
      el.removeEventListener("touchcancel", onTouchEnd);
    };
  });
</script>

<div class="grid-root">
  <div
    class="viewport"
    id="photo-grid-scroll"
    role="grid"
    aria-label="Photo grid"
    aria-activedescendant={session.focused ? ACTIVE_CELL_ID : undefined}
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
    oncontextmenu={onContextMenu}
  >
  <div
    class="canvas"
    class:animate={animateCells}
    bind:this={canvasEl}
    style="height:{contentHeight}px; transform: translateY({pullY}px); transition:{pullDragging ? 'none' : 'transform 0.28s cubic-bezier(0.22, 1, 0.36, 1)'}"
  >
    {#each visibleHeaders as h (h.key)}
      <div
        class="group-header"
        class:sub={h.depth > 0}
        style="transform: translateY({h.y}px); height:{h.h}px; padding-left:{padX + h.depth * 14}px"
      >
        <button
          class="gh-collapse"
          class:collapsed={h.collapsed}
          title={h.collapsed ? "Expand" : "Collapse"}
          aria-label={h.collapsed ? "Expand section" : "Collapse section"}
          onpointerdown={(e) => e.stopPropagation()}
          onclick={(e) => {
            e.stopPropagation();
            toggleCollapsed(h.key);
          }}
        >
          <ChevronRight size={h.depth > 0 ? 12 : 14} />
        </button>
        <span class="gh-label">{h.label}</span>
        <span class="gh-count">{h.count}</span>
        <span class="gh-line"></span>
        <input
          class="gh-check"
          type="checkbox"
          title="Select all in this section"
          aria-label="Select all in this section"
          checked={groupAllSelected(h)}
          onpointerdown={(e) => e.stopPropagation()}
          onclick={(e) => {
            e.stopPropagation();
            toggleGroupSelect(h);
          }}
        />
      </div>
    {/each}
    {#each visible as v (v.item.id)}
      {@const selected = session.selectedIds.has(v.item.id)}
      {@const inset = selected ? SELECTED_INSET : 0}
      {@const dims = displayDims(v.item)}
      {@const stacked = v.span > 1}
      {@const wholePhoto = settings.gridPhotoFit === "fit" && dims !== null}
      {@const burstAt = stacked ? null : session.burstPositionAt(v.first)}
      {@const isFocused =
        session.focusedIndex >= v.first &&
        session.focusedIndex < v.first + v.span &&
        session.selectedIds.size === 0}
      <div
        class="cell"
        class:focused={isFocused}
        class:selected
        style="transform: translate({v.x + GAP / 2 + inset}px, {v.y + GAP / 2 + inset}px); width:{CELL - GAP - inset * 2}px; height:{CELL - GAP - inset * 2}px"
        {...isFocused ? { id: ACTIVE_CELL_ID } : {}}
        role="button"
        tabindex="-1"
      >
        <div
          class="frame"
          class:stacked
          class:loading={!loaded.has(v.item.id) &&
            !v.item.thumbFailed &&
            !(v.item.kind === 2 && posterFailed.has(v.item.id))}
          class:pending={!loaded.has(v.item.id) && !v.item.thumbReady && v.item.kind !== 2}
        >
          <!-- Cards peeking out behind a collapsed burst. They sit in the
               padding .frame.stacked opens up, so the deck never leaves the
               cell and never touches its neighbours. -->
          {#if stacked}
            {@const behind = session.filtered.slice(
              v.first + 1,
              v.first + Math.min(v.span, 3),
            )}
            <!-- One card per frame it actually hides, capped at two: a 2-shot
                 burst that showed three cards would misreport its own size. -->
            {#if behind[1]}
              <span class="deck d2"><img src={thumbUrl(behind[1])} alt="" decoding="async" /></span>
            {/if}
            {#if behind[0]}
              <span class="deck d1"><img src={thumbUrl(behind[0])} alt="" decoding="async" /></span>
            {/if}
          {/if}
          <!-- Fitted dimensions keep the badges inside the visible photo. -->
          <div
            class="photo"
            class:fit={settings.gridPhotoFit === "fit"}
            class:queued={settings.dimQueuedDeletes &&
              (v.item.flag === -1 || session.pendingDeleteIds.has(v.item.id))}
            style={wholePhoto && dims
              ? `${dims.h > dims.w ? "width:auto" : "height:auto"}; aspect-ratio:${dims.w}/${dims.h}`
              : ""}
          >
            {#if v.item.kind === 2}
              {#if posterFailed.has(v.item.id)}
                <!-- No pregenerated poster (no extractor / undecodable): a
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
              <div class="unreadable" title="{v.item.name}.{v.item.ext} could not be decoded">
                <FileWarning size={22} />
                <span>{v.item.ext.toUpperCase()}</span>
              </div>
            {:else}
              <img
                src={thumbSrc(v.item)}
                alt=""
                decoding="async"
                draggable="false"
                loading="eager"
                onload={() => loaded.add(v.item.id)}
                onerror={() => retryThumb(v.item.id)}
              />
            {/if}
            <!-- Spins for as long as this photo genuinely lacks a preview.
                 It used to also require the background pass to be running,
                 which meant the spinner vanished the moment the pass finished
                 (or had not restarted yet after a half-finished import) even
                 though the preview still was not there. `previewFailed` is what
                 stops it now: a real answer rather than a proxy. -->
            {#if v.item.kind !== 2 && !v.item.thumbFailed && !v.item.previewFailed && loaded.has(v.item.id) && !previewReady.has(v.item.id)}
              <span class="preview-spin" title="Generating full preview…">
                <Loader size={12} />
              </span>
            {/if}
            {#if v.item.label}
              <span class="label-bar" title={formatColorLabel(v.item.label)} style:border-color={labelColors[v.item.label]}></span>
            {/if}
            {#if stacked}
              <span class="burst" title="Burst of {v.span} photos">
                <Layers size={10} />{v.span}
              </span>
            {:else if burstAt}
              <span
                class="burst"
                title="Shot {burstAt.position} of a burst of {burstAt.total}"
              >
                <Layers size={10} />{burstAt.position}/{burstAt.total}
              </span>
            {/if}
            {#if session.mirrorMode && v.item.groupSize > 1}
              {@const pair = session.pairHalves(v.item)}
              <span
                class="chip pair"
                class:split={v.item.decoupled}
                class:below-burst={stacked || burstAt}
                title={pair ? pair.halves.map(describeHalf).join(" · ") : undefined}
              >
                {#if v.item.decoupled}
                  <Scissors size={10} /><span>SPLIT</span>
                {:else if pair?.diverged}
                  <!-- Halves shown separately only once they disagree, so the
                       ordinary pair keeps drawing exactly as it always has. -->
                  {#each pair.halves as h, hi (h.id)}
                    {#if hi > 0}<span class="half-sep"></span>{/if}
                    <span class="half" class:struck={h.queuedDelete}>{h.name}</span>
                  {/each}
                {:else}
                  RAW+JPG
                {/if}
              </span>
            {:else if v.item.kind === 0}
              <span class="chip raw" class:below-burst={stacked || burstAt}>RAW</span>
            {/if}
            {#if stacked}
              <!-- A stack stands for many photos, so the first frame's flag would
                   speak for frames it knows nothing about. Sum them instead: a
                   burst with some picks and some rejects is one that has been
                   worked, not one in conflict. -->
              {@const sum = stackSummary(v.first, v.span)}
              {#if sum.picks > 0 || sum.rejects > 0}
                <span class="badge counts" title={stackTitle(sum, v.span)}>
                  {#if sum.picks > 0}
                    <span class="pick"><Check size={11} />{sum.picks}</span>
                  {/if}
                  {#if sum.rejects > 0}
                    <span class="reject"><X size={11} />{sum.rejects}</span>
                  {/if}
                </span>
              {/if}
              {#if sum.maxRating > 0}
                <span class="stars" title="Best rating in this burst">
                  {"★".repeat(sum.maxRating)}
                </span>
              {/if}
            {:else}
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
  {#if stickyHeader}
    <!-- Pinned copy of the current group path: rides the top of the viewport
         (cells scroll under it) so the full hierarchy and count stay visible. -->
    <div class="sticky-group" style="padding-left:{padX}px">
      <span class="sg-label">{stickyHeader.label}</span>
      <span class="sg-count">{stickyHeader.count}</span>
    </div>
  {/if}
  <!-- Native bar hidden on .viewport below — matches the folder tree/filmstrip
       convention (no 8px carved out of the grid's own width for a scrollbar
       gutter); this floats over the content instead. -->
  <OverlayScrollbar
    orientation="vertical"
    viewport={height}
    content={contentHeight}
    position={scrollTop}
    controls="photo-grid-scroll"
    onSeek={(pos) => {
      if (viewport) viewport.scrollTop = pos;
    }}
  />
  {#if pullY > 0 || refreshing}
    <div
      class="pull-indicator"
      class:refreshing
      class:armed={!refreshing && pullY >= PULL_THRESHOLD}
      style="transform: translateY({pullY}px); opacity:{Math.min(1, pullY / PULL_THRESHOLD)}; transition:{pullDragging
        ? 'none'
        : 'transform 0.28s cubic-bezier(0.22, 1, 0.36, 1), opacity 0.2s'}"
    >
      <!-- Static while pulling; the spin animation begins only on release (the
           `refreshing` class), so it never snaps a pull-proportional angle back
           to zero at the moment the rescan fires. -->
      <span class="pull-spin">
        <Loader size={18} />
      </span>
    </div>
  {/if}
  {#if bgStatus}
    <div class="loading-pill" role="status" aria-live="polite">
      <span class="spin"><Loader size={13} /></span>
      <span>{bgStatus}</span>
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  /* Positioned wrapper so the loading pill can float over the scrolling grid
     without scrolling away with the cells. */
  .grid-root {
    position: relative;
    flex: 1;
    display: flex;
    min-height: 0;
    min-width: 0;
    /* Clip the pull-to-refresh spinner (anchored at top:-44px) to the grid area
       so its portion above the grid's top edge stays hidden under the toolbar —
       it appears to emerge from beneath the header instead of over it. The
       OverlayScrollbar and loading-pill both sit within these bounds. */
    overflow: hidden;
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

  /* Rides above the bottom action bar whenever it is up (selection mode), so
     the bar never buries the only report of what the background is doing.
     --bottom-bar-h is measured and published by the page shell; it is 0 while
     no bar is shown. */
  .loading-pill {
    position: absolute;
    right: 12px;
    bottom: calc(12px + var(--bottom-bar-h, 0px));
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

  /* Pull-to-refresh spinner: rides in the gap opened above the grid content as
     the finger drags down (the .canvas translates with it). Anchored just above
     the top edge so translateY(pullY) slides it into view. */
  .pull-indicator {
    position: absolute;
    top: -44px;
    left: 0;
    right: 0;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 5;
    pointer-events: none;
    color: #8a8a93;
  }

  /* Past the release threshold: switch to the brand accent so the user knows a
     release will now trigger the refresh. */
  .pull-indicator.armed,
  .pull-indicator.refreshing {
    color: var(--accent);
  }

  .pull-spin {
    display: inline-flex;
  }

  .pull-indicator.refreshing .pull-spin {
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

  /* Group-by section header: a full-width band positioned by transform like the
     cells. Depth 0 is bolder; deeper levels are indented (inline padding-left)
     and lighter. */
  .group-header {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    display: flex;
    /* Centre every child (chevron, label, count, divider line, checkbox) on one
       shared midline. Aligning a mix of different-height boxes — a 20px icon
       button, 14px checkbox, ~14px text line, 1px rule — by any edge instead
       leaves them visibly off from each other; centring is the one rule under
       which they all read as sitting on the same line, with no per-element
       nudging. The label is bottom-padded so the header hugs the row of cells
       just beneath it rather than floating mid-band. */
    align-items: center;
    padding-bottom: 8px;
    gap: 8px;
    padding-right: 12px;
    box-sizing: border-box;
    font-size: 12px;
    font-weight: 600;
    color: #d5d5db;
    user-select: none;
    pointer-events: none;
  }

  .group-header.sub {
    font-weight: 500;
    color: #9a9aa2;
    font-size: 11px;
  }

  .gh-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .gh-count {
    color: #6a6a72;
    font-weight: 500;
  }

  .gh-line {
    flex: 1;
    height: 1px;
    background: var(--border);
  }

  /* Collapse toggle: left of the label. Centred with the row (align-items:
     center on the header), so no per-element alignment override is needed.
     Pointer-events restored since the header itself passes clicks through to
     the grid beneath. */
  .gh-collapse {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: inherit;
    cursor: pointer;
    pointer-events: auto;
    transform: rotate(90deg); /* expanded: chevron points down */
    transition: transform 0.12s ease;
  }

  .gh-collapse.collapsed {
    transform: rotate(0deg); /* collapsed: chevron points right */
  }

  .gh-collapse:hover {
    background: var(--hover);
  }

  /* Group select-all checkbox: right end of the row, deliberately small (not
     styled as a big touch target) so it reads as a light-touch bulk-select
     affordance, not a primary action. */
  .gh-check {
    flex: none;
    width: 14px;
    height: 14px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
    pointer-events: auto;
  }

  /* Pinned current-group header (optional). A solid bar at the top of the grid
     that cells scroll beneath; mirrors the depth-0 header look. */
  .sticky-group {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 12px;
    box-sizing: border-box;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    font-weight: 600;
    color: #d5d5db;
    z-index: 4;
    pointer-events: none;
  }

  .sg-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sg-count {
    color: #6a6a72;
    font-weight: 500;
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

  /* Animates the selection shrink (width/height/position all move together by
     SELECTED_INSET). Scoped to `.animate` because the same properties carry the
     layout: on a column-count or cell-size change every cell moves at once, and
     animating that reads as the grid reordering itself rather than as feedback.
     A mounted cell's x/y is invariant under scrolling, so this never fires on
     scroll either way. */
  .canvas.animate .cell {
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

  /* Collapsed burst: the thumbnail gives up a strip along its top and right
     edges so the two cards behind it have somewhere to peek from. */
  .frame.stacked {
    box-sizing: border-box;
    padding: 8px 8px 0 0;
  }

  /* Both deck cards trace the photo's own box (the frame minus that padding)
     and are then nudged out of it. .photo is position: relative, so DOM order
     keeps the photo on top of them. */
  .deck {
    position: absolute;
    top: 8px;
    right: 8px;
    bottom: 0;
    left: 0;
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    background: var(--surface-2);
    overflow: hidden;
  }

  /* The cards carry the real frames they stand for. They are darkened so the
     front photo still reads as the one in charge of the cell. */
  .deck img {
    filter: brightness(0.55);
  }

  .deck.d1 {
    transform: translate(4px, -4px);
  }

  .deck.d2 {
    transform: translate(8px, -8px);
    background: var(--surface);
  }

  .deck.d2 img {
    filter: brightness(0.4);
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

  .photo.fit img {
    object-fit: contain;
  }

  /* Subtle shimmer while a cell's thumbnail is still being generated/decoded.
     It's the frame's own background (behind .photo and its chips), so chips
     stay on top and a finished portrait cell shows the grid's dark background
     through its letterbox gutters, not a gray box. */
  /* No image yet. A flat placeholder, because for a thumbnail that already
     exists this lasts a frame or two and a shimmer would just be a flicker. */
  .frame.loading {
    background: var(--surface-2);
  }

  /* No image AND none generated yet: this one really is waiting on work, so it
     says so for as long as that lasts. */
  .frame.loading.pending {
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

  /* Video with no generated poster (no extractor / undecodable clip). Reads
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

  /* A pair whose halves disagree. Deliberately the same colour as an ordinary
     pair: queueing the RAW and keeping the JPEG is a normal way to work, so
     this reports the difference rather than warning about it. */
  .chip.pair .half-sep {
    width: 1px;
    align-self: stretch;
    margin: 1px -1px;
    background: currentColor;
    opacity: 0.4;
  }

  .chip.pair .half.struck {
    text-decoration: line-through;
    opacity: 0.55;
  }

  /* Second row of the top-left stack: the burst badge owns the corner. */
  .chip.below-burst {
    top: 24px;
  }

  /* Burst badge: the same pill as the loupe's info bar, sized for a cell. It
     reads "how many" on a collapsed stack and "which one" on a loose frame. */
  .burst {
    position: absolute;
    top: 4px;
    left: 4px;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 10px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid var(--border-strong);
    background: rgba(0, 0, 0, 0.55);
    color: #e8e8e8;
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

  /* Pick/reject tallies on a collapsed burst, in the single badge's place. */
  .badge.counts {
    gap: 5px;
    font-size: 10px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .badge.counts > span {
    display: inline-flex;
    align-items: center;
    gap: 1px;
  }

  .badge.counts .pick {
    color: #6be675;
  }

  .badge.counts .reject {
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
