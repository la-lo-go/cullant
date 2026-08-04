<script lang="ts">
  /**
   * The N-up view: compare and survey are one thing.
   *
   * Compare used to be two fixed panes and survey an unrelated grid of buttons,
   * so the one job they share — reduce a burst to the frame worth keeping — was
   * done twice and badly. This draws N cells, and N is the only difference
   * between the two doors into it: `C` opens with two, `N` with the whole
   * selection or burst.
   *
   * Eliminating a cell pulls the next candidate into the hole it leaves (see
   * `session.surveyPool`), so two cells IS the tournament: the survivor stays put
   * and meets the next challenger. That is why pinning is gone — only the cell
   * you empty gets refilled, which is what pinning was for.
   *
   * Every cell is a real `ZoomImage` WITHOUT `standalone`, so they share one zoom
   * and pan through the `view` store. Judging sharpness between two frames of a
   * burst is the whole point of putting them side by side, and it could not be
   * done while each pane zoomed on its own.
   */
  import { untrack } from "svelte";
  import { previewUrl } from "../api";
  import { session } from "../stores/session.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import { settings } from "../stores/settings.svelte";
  import { view } from "../stores/view.svelte";
  import { tags } from "../stores/tags.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import Filmstrip from "./Filmstrip.svelte";
  import X from "@lucide/svelte/icons/x";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Minimize from "@lucide/svelte/icons/minimize";
  import Layers from "@lucide/svelte/icons/layers";
  import { edgeBounce } from "../anim";

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  const items = $derived(session.surveyItems);

  /** Roughly square. As the field narrows the survivors get a bigger share of
   *  the screen, which is the reward for eliminating one; two cells land side by
   *  side, which is what compare has always looked like. */
  const cols = $derived(Math.max(1, Math.ceil(Math.sqrt(items.length))));

  let cellsEl = $state<HTMLElement | null>(null);
  // Bounce the cells when stepping past the first/last photo. Track the
  // last-seen bump so switching into this view doesn't replay a stale one.
  let lastBump = session.edgeBump.n;
  $effect(() => {
    const b = session.edgeBump;
    if (b.n === lastBump || !cellsEl) return;
    lastBump = b.n;
    edgeBounce(cellsEl, b.dir, "x");
  });

  /** Each cell pages ITSELF, so stepping the photo in one never moves another. */
  function pageCell(index: number) {
    return (dir: number) => session.pageSurveyCell(index, dir);
  }

  // Warm the cache one candidate ahead so a long burst never stalls when a cell
  // is refilled. Gated by a dwell, like the loupe: fast elimination must not
  // flood the decode pool.
  const AHEAD_DWELL_MS = 1000;
  $effect(() => {
    const next = session.surveyPool[0];
    const timer = setTimeout(() => {
      const item = untrack(() => catalog.items.find((i) => i.id === next));
      if (item && item.kind !== 2 && !untrack(() => catalog.previewReady.has(item.id))) {
        new Image().src = previewUrl(item);
      }
    }, AHEAD_DWELL_MS);
    return () => clearTimeout(timer);
  });
</script>

<div class="multi">
  {#if !view.fullscreen}
    <div class="bar">
      <span class="count">
        {items.length} on screen
        {#if session.surveyPool.length > 0}
          <span class="queued">· {session.surveyPool.length} waiting</span>
        {/if}
        {#if session.surveyRequested > 0}
          <span class="queued">· first {items.length} of {session.surveyRequested} selected</span>
        {/if}
      </span>
      <span class="hint">
        ← → to move · X to {settings.surveyEliminate === "reject" ? "reject" : "drop"} · Esc to leave
      </span>
      <button
        class="icon-btn"
        title="Full screen"
        aria-label="Full screen"
        onclick={(e) => {
          view.toggleFullscreen();
          (e.currentTarget as HTMLElement).blur();
        }}
      >
        <Maximize size={15} />
      </button>
      <button
        class="icon-btn"
        onclick={() => session.closeSurvey()}
        aria-label="Back to grid"
        title="Back to grid (Esc)"
      >
        <X size={15} />
      </button>
    </div>
  {:else}
    <button
      class="icon-btn floating"
      title="Exit full screen"
      aria-label="Exit full screen"
      onclick={(e) => {
        view.toggleFullscreen();
        (e.currentTarget as HTMLElement).blur();
      }}
    >
      <Minimize size={15} />
    </button>
  {/if}

  <div
    class="cells"
    bind:this={cellsEl}
    style="grid-template-columns: repeat({cols}, minmax(0, 1fr))"
  >
    {#each items as item, i (item.id)}
      {@const focused = session.focused?.id === item.id}
      <div class="cell" class:focused>
        <!-- Clicking a cell focuses it. Underneath the photo, because ZoomImage
             owns the pointer for pan, pinch and the radial menu; this only sees
             what falls through to it. -->
        <button
          class="focus-catch"
          aria-label="Focus {item.name}"
          onpointerdown={() => session.focusSurveyItem(item.id)}
        ></button>
        <!-- No `standalone`: every cell shares the one zoom in the view store,
             which is what makes an A/B of sharpness possible at all. -->
        <ZoomImage {item} onPage={pageCell(i)} />
        {#if !view.fullscreen}
          {@const b = session.burstPositionOf(item.id)}
          <span class="caption" class:focused-caption={focused}>
            <span style:color={item.label ? labelColors[item.label] : null}>
              {item.name}.{item.ext}
            </span>
            {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
            {#if b}
              <span class="burst" title="Shot {b.position} of a burst of {b.total}">
                <Layers size={10} />{b.position}/{b.total}
              </span>
            {/if}
            {#each item.tagIds as tagId (tagId)}
              {@const t = tags.byId.get(tagId)}
              {#if t}
                {@const c = t.color ?? "#888"}
                <span class="tagpill" style="border-color: {c}; background: {c}2e">{t.name}</span>
              {/if}
            {/each}
          </span>
        {/if}
      </div>
    {/each}
  </div>

  {#if session.showFilmstrip && !view.fullscreen}
    <!-- The whole visible set, not just the cells: the strip is how you see
         where the field sits inside the shoot. The focused cell is the one it
         marks; with N cells there is no single "companion" left to mark. -->
    <Filmstrip items={session.filtered} />
  {/if}
</div>

<style>
  .multi {
    position: relative;
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 8px;
    font-size: 12px;
    color: #bbb;
  }

  .count {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .queued {
    opacity: 0.55;
  }

  .hint {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.55;
  }

  .icon-btn {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 24px;
    padding: 0;
    background: none;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: #bbb;
    cursor: pointer;
  }

  .icon-btn:hover {
    color: #fff;
  }

  .icon-btn.floating {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 3;
    background: rgba(0, 0, 0, 0.5);
  }

  .cells {
    flex: 1;
    min-height: 0;
    display: grid;
    gap: 2px;
  }

  .cell {
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    border: 2px solid transparent;
    border-radius: 4px;
    overflow: hidden;
  }

  .cell.focused {
    border-color: rgba(var(--accent-rgb), 0.55);
  }

  .focus-catch {
    position: absolute;
    inset: 0;
    background: none;
    border: none;
    padding: 0;
    cursor: pointer;
  }

  .caption {
    position: absolute;
    left: 6px;
    bottom: 6px;
    display: flex;
    align-items: center;
    gap: 6px;
    max-width: calc(100% - 12px);
    padding: 2px 7px;
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.55);
    color: #ddd;
    font-size: 11px;
    pointer-events: none;
  }

  .caption.focused-caption {
    color: #fff;
    box-shadow: inset 0 0 0 1px rgba(var(--accent-rgb), 0.5);
  }

  .stars {
    color: #ffd166;
    flex: none;
  }

  .burst {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    flex: none;
    font-variant-numeric: tabular-nums;
    opacity: 0.85;
  }

  .tagpill {
    flex: none;
    padding: 0 5px;
    border: 1px solid;
    border-radius: 999px;
    font-size: 10px;
  }
</style>
