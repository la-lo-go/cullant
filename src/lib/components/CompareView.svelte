<script lang="ts">
  import { session } from "../stores/session.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import Filmstrip from "./Filmstrip.svelte";

  // 2-up compare: the focused photo against the next one in filter order.
  // Zoom/pan state lives in the shared view store, so both panes stay in
  // sync — the sharpness A/B between near-duplicates.
  const left = $derived(session.focused);
  const right = $derived(session.filtered[session.focusedIndex + 1]);
</script>

<div class="compare">
  <div class="panes">
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
