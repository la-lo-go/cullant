<script lang="ts">
  /** Draws the state of a grip drag: the compass buttons. The ghosts and frames come from `DockGhost`. */
  import PanelTop from "@lucide/svelte/icons/panel-top";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import { COMPASS_BUTTON, DOCK_SIDES, compassButtons, dock } from "../stores/dock.svelte";

  const SIDE_ICONS = { top: PanelTop, right: PanelRight, bottom: PanelBottom, left: PanelLeft };
  const drag = $derived(dock.drag);
</script>

{#if drag}
  {@const { region, candidate } = drag}
  <div class="layer" aria-hidden="true">
    <div
      class="compass"
      style:left="{region.left + region.width / 2}px"
      style:top="{region.top + region.height / 2}px"
    >
      {#each compassButtons(region, DOCK_SIDES[drag.panel]) as { side, x, y } (side)}
        {@const Icon = SIDE_ICONS[side]}
        <div
          class="target"
          class:active={candidate === side}
          style:width="{COMPASS_BUTTON}px"
          style:height="{COMPASS_BUTTON}px"
          style:left="{x - (region.left + region.width / 2) - COMPASS_BUTTON / 2}px"
          style:top="{y - (region.top + region.height / 2) - COMPASS_BUTTON / 2}px"
        >
          <Icon size={18} />
        </div>
      {/each}
    </div>
  </div>
{/if}

<style>
  .layer {
    position: fixed;
    inset: 0;
    z-index: 200;
    pointer-events: none;
  }
  .compass {
    position: absolute;
    width: 0;
    height: 0;
    animation: compass-in 120ms ease-out;
  }
  .target {
    position: absolute;
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: #232329;
    color: #c9c9d1;
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.45);
    transition:
      background 100ms ease,
      color 100ms ease,
      border-color 100ms ease;
  }
  .target.active {
    border-color: var(--accent);
    background: var(--accent);
    color: #0f1514;
  }
  @keyframes compass-in {
    from {
      opacity: 0;
      transform: scale(0.85);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .target {
      transition: none;
    }
    .compass {
      animation: none;
    }
  }
</style>
