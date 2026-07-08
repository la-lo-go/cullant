<script lang="ts">
  import { thumbUrl, type ItemLite } from "../api";
  import { session } from "../stores/session.svelte";

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

  const visible = $derived.by(() => {
    const out: { item: ItemLite; index: number; x: number }[] = [];
    for (let i = first; i < last; i++) {
      out.push({ item: items[i], index: i, x: i * CELL });
    }
    return out;
  });

  // Keep the focused frame centered as the user arrows through the shoot.
  $effect(() => {
    if (!strip) return;
    const target = session.focusedIndex * CELL - width / 2 + CELL / 2;
    strip.scrollTo({ left: Math.max(0, target) });
  });
</script>

<div
  class="strip"
  bind:this={strip}
  bind:clientWidth={width}
  onscroll={() => strip && (scrollLeft = strip.scrollLeft)}
>
  <div class="canvas" style="width:{items.length * CELL}px">
    {#each visible as v}
      <div
        class="cell"
        class:focused={v.index === session.focusedIndex}
        style="transform: translateX({v.x}px); width:{CELL}px"
        onpointerdown={() => (session.focusedIndex = v.index)}
        role="button"
        tabindex="-1"
      >
        <img src={thumbUrl(v.item)} alt="" decoding="async" draggable="false" />
        {#if v.item.flag !== 0}
          <span class="dot" class:pick={v.item.flag === 1} class:reject={v.item.flag === -1}></span>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .strip {
    height: 104px;
    flex: none;
    overflow-x: auto;
    overflow-y: hidden;
    background: #1e1e23;
    border-top: 1px solid #2e2e36;
  }

  .canvas {
    position: relative;
    height: 100%;
  }

  .cell {
    position: absolute;
    top: 0;
    height: 100%;
    padding: 6px 3px;
    box-sizing: border-box;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 6px;
  }

  .cell.focused {
    outline: 2px solid #6b8bff;
    outline-offset: -2px;
  }

  img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
    border-radius: 3px;
    user-select: none;
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
