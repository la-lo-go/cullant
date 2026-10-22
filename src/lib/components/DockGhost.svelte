<script lang="ts">
  /**
   * What a grip drag shows for one panel: at each side it can dock to, a frame
   * with a ghost of the panel in it, at the size and orientation it will have
   * when it lands. The host passes the ghost as a snippet. The slot of the
   * candidate side opens from the outer edge while the content makes its gap;
   * the ghost keeps its full size and is only revealed, never scaled.
   * The slot of the panel's own side shows the frame alone: the panel is there.
   */
  import { untrack, type Snippet } from "svelte";
  import {
    DOCK_SIDES,
    dock,
    isVertical,
    previewRect,
    type DockDrag,
    type DockPanel,
    type Side,
  } from "../stores/dock.svelte";

  const { panel, ghost }: { panel: DockPanel; ghost: Snippet<[Side, number]> } = $props();

  const drag = $derived(dock.drag?.panel === panel ? dock.drag : null);
  // After a cancel the slots close with the gap, so the last geometry stays for one motion.
  let last = $state.raw<DockDrag | null>(null);
  $effect(() => {
    if (drag) last = drag;
    else if (!shown) last = null;
  });
  const geometry = $derived(drag ?? last);
  const shown = $derived(drag !== null || dock.returning === panel);

  // The ghosts of the other sides mount one frame after another, after the first paint of
  // the drag, so no single frame pays for all of them. The candidate side always shows its own.
  let ready = $state<Side[]>([]);
  $effect(() => {
    if (!shown) {
      ready = [];
      return;
    }
    const todo = DOCK_SIDES[panel].filter((side) => side !== untrack(() => dock[panel]));
    let frame = 0;
    const next = () => {
      const side = todo.shift();
      if (!side) return;
      ready = [...ready, side];
      frame = requestAnimationFrame(next);
    };
    frame = requestAnimationFrame(next);
    return () => cancelAnimationFrame(frame);
  });

  // The layer sits on the body: a transformed ancestor would turn `fixed` into `absolute`.
  function toBody(node: HTMLElement) {
    document.body.appendChild(node);
    return { destroy: () => node.remove() };
  }

  /** Anchored to the outer edge, so the slot grows from it. */
  function slotStyle(region: DOMRect, side: Side, size: number, on: boolean): string {
    const r = previewRect(region, side, size);
    const grown = isVertical(side) ? `width:${on ? r.width : 0}px;height:${r.height}px` : `width:${r.width}px;height:${on ? r.height : 0}px`;
    const origin = `left:${r.left}px;top:${r.top}px`;
    const anchor = {
      left: origin,
      top: origin,
      right: `right:calc(100% - ${r.left + r.width}px);top:${r.top}px`,
      bottom: `left:${r.left}px;bottom:calc(100% - ${r.top + r.height}px)`,
    }[side];
    return `${anchor};${grown}`;
  }

  function ghostStyle(side: Side, size: number, region: DOMRect): string {
    const r = previewRect(region, side, size);
    const anchor = { left: "left:0;top:0", top: "left:0;top:0", right: "right:0;top:0", bottom: "left:0;bottom:0" }[side];
    return `${anchor};width:${r.width}px;height:${r.height}px`;
  }
</script>

{#if shown && geometry}
  {@const region = geometry.region}
  <div class="ghosts" aria-hidden="true" inert use:toBody>
    {#each DOCK_SIDES[panel] as side (side)}
      {@const size = geometry.thickness(side)}
      {@const on = drag?.candidate === side}
      <div class="slot" class:on style={slotStyle(region, side, size, on)}>
        {#if side !== dock[panel] && (on || ready.includes(side))}
          <div class="ghost" style={ghostStyle(side, size, region)}>
            {@render ghost(side, size)}
          </div>
        {/if}
        <div class="frame"></div>
      </div>
    {/each}
  </div>
{/if}

<style>
  /* Under the compass of DockGuides. */
  .ghosts {
    position: fixed;
    inset: 0;
    z-index: 199;
    pointer-events: none;
  }
  .slot {
    position: absolute;
    overflow: hidden;
    transition:
      width var(--dock-motion),
      height var(--dock-motion);
  }
  .ghost {
    position: absolute;
    display: flex;
  }
  /* Parts such as the scrollbar track set their own pointer-events. */
  .ghost :global(*) {
    pointer-events: none !important;
  }
  .frame {
    position: absolute;
    inset: 0;
    box-sizing: border-box;
    border: 2px solid var(--accent);
    border-radius: var(--r-md);
    background: rgba(var(--accent-rgb), 0.08);
  }
</style>
