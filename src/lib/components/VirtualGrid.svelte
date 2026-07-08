<script lang="ts">
  import { thumbUrl, videoUrl, type ItemLite } from "../api";
  import { session } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import { view } from "../stores/view.svelte";

  let { items }: { items: ItemLite[] } = $props();

  const CELL = 200; // cell pitch in px (thumbnail + label + gap)
  const OVERSCAN_ROWS = 2;

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  let viewport = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let width = $state(0);
  let height = $state(0);

  const cols = $derived(Math.max(1, Math.floor(width / CELL)));
  const totalRows = $derived(Math.ceil(items.length / cols));
  const firstRow = $derived(Math.max(0, Math.floor(scrollTop / CELL) - OVERSCAN_ROWS));
  const lastRow = $derived(
    Math.min(totalRows, Math.ceil((scrollTop + height) / CELL) + OVERSCAN_ROWS)
  );

  // Report layout so keyboard ↑/↓ move one visual row.
  $effect(() => {
    session.gridCols = cols;
  });

  // Keep the focused cell in view when keyboard navigation moves it.
  $effect(() => {
    const row = Math.floor(session.focusedIndex / cols);
    if (!viewport) return;
    const top = row * CELL;
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
        out.push({ item: items[index], index, x: col * CELL, y: row * CELL });
      }
    }
    return out;
  });

  function onScroll() {
    if (viewport) scrollTop = viewport.scrollTop;
  }
</script>

<div
  class="viewport"
  bind:this={viewport}
  bind:clientWidth={width}
  bind:clientHeight={height}
  onscroll={onScroll}
>
  <div class="canvas" style="height:{totalRows * CELL}px">
    {#each visible as v}
      <div
        class="cell"
        class:focused={v.index === session.focusedIndex}
        style="transform: translate({v.x}px, {v.y}px); width:{CELL}px; height:{CELL}px"
        onpointerdown={() => (session.focusedIndex = v.index)}
        ondblclick={() => (view.mode = "viewer")}
        role="button"
        tabindex="-1"
      >
        <div class="frame" style:--label-color={v.item.label ? labelColors[v.item.label] : "transparent"}>
          {#if v.item.kind === 2}
            <!-- preload=metadata shows the first frame; only ~30 cells live -->
            <video src={videoUrl(v.item)} preload="metadata" muted></video>
            <span class="chip video">▶</span>
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
              {v.item.decoupled ? "✂ SPLIT" : "RAW+JPG"}
            </span>
          {:else if v.item.kind === 0}
            <span class="chip raw">RAW</span>
          {/if}
          {#if session.pendingDeleteIds.has(v.item.id)}
            <span class="badge pending" title="Queued for deletion">🗑</span>
          {:else if v.item.flag !== 0}
            <span class="badge" class:pick={v.item.flag === 1} class:reject={v.item.flag === -1}>
              {v.item.flag === 1 ? "✔" : "✖"}
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
        <span class="name">{v.item.name}.{v.item.ext}</span>
      </div>
    {/each}
  </div>
</div>

<style>
  .viewport {
    flex: 1;
    overflow-y: auto;
    overflow-x: hidden;
    contain: strict;
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
  }

  .cell.focused {
    outline: 2px solid #6b8bff;
    outline-offset: -2px;
    background: rgba(107, 139, 255, 0.08);
  }

  .frame {
    position: relative;
    flex: 1;
    min-height: 0;
    background: #26262c;
    border-radius: 6px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    border-bottom: 3px solid var(--label-color);
  }

  img,
  video {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    user-select: none;
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
    font-size: 12px;
    padding: 1px 4px;
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
