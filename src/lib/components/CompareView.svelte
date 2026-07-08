<script lang="ts">
  import { previewUrl } from "../api";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import Filmstrip from "./Filmstrip.svelte";
  import X from "@lucide/svelte/icons/x";

  // 2-up compare: the focused photo against the next one in filter order.
  // Zoom/pan state lives in the shared view store, so both panes stay in
  // sync — the sharpness A/B between near-duplicates.
  const left = $derived(session.focused);
  const right = $derived(session.filtered[session.focusedIndex + 1]);

  // Warm the cache one photo ahead of the right pane, so arrowing quickly
  // through a burst always finds the next preview already decoded.
  $effect(() => {
    const ahead = session.filtered[session.focusedIndex + 2];
    if (ahead && ahead.kind !== 2) {
      new Image().src = previewUrl(ahead);
    }
  });
</script>

<div class="compare">
  <div class="panes">
    <button class="back" title="Back to grid (Esc)" onclick={() => (view.mode = "grid")}><X size={16} /></button>
    {#if left}
      <div class="pane">
        <ZoomImage item={left} />
        <span class="caption focused-caption">{left.name}.{left.ext}</span>
      </div>
    {/if}
    {#if right}
      <div class="pane">
        <ZoomImage item={right} />
        <span class="caption">{right.name}.{right.ext}</span>
      </div>
    {:else}
      <div class="pane empty">End of set</div>
    {/if}
  </div>
  <Filmstrip items={session.filtered} />
</div>

<style>
  .compare {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .panes {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 2px;
    position: relative;
  }

  .back {
    position: absolute;
    top: 10px;
    right: 10px;
    z-index: 5;
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 6px;
    padding: 0;
    font-size: 14px;
    font-family: inherit;
    background: rgba(0, 0, 0, 0.45);
    color: rgba(255, 255, 255, 0.75);
    cursor: pointer;
  }

  .back:hover {
    background: rgba(0, 0, 0, 0.7);
    color: #fff;
  }

  .pane {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }

  .caption {
    position: absolute;
    bottom: 6px;
    left: 10px;
    font-size: 12px;
    opacity: 0.7;
    background: rgba(0, 0, 0, 0.5);
    padding: 2px 8px;
    border-radius: 4px;
    pointer-events: none;
  }

  .focused-caption {
    outline: 1px solid #6b8bff;
  }

  .pane.empty {
    align-items: center;
    justify-content: center;
    opacity: 0.4;
  }
</style>
