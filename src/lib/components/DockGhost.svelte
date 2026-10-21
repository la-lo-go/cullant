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

  const LINGER_MS = 120;
  const { panel, ghost }: { panel: DockPanel; ghost: Snippet<[Side, number]> } = $props();

  const drag = $derived(dock.drag?.panel === panel ? dock.drag : null);
  // After a cancel the slots close with the gap, so the last geometry stays for one motion.
  let last = $state.raw<DockDrag | null>(null);
  $effect(() => {
    if (drag) last = drag;
  });
  const geometry = $derived(drag ?? last);
  const active = $derived(drag !== null || dock.returning === panel);
  // After a drop the layer hides at once and is removed later, so tearing the ghosts
  // down does not share a frame with the landing of the real panel.
  let lingering = $state(false);
  $effect(() => {
    if (active) {
      lingering = true;
      return;
    }
    const timer = setTimeout(() => (lingering = false), LINGER_MS);
    return () => clearTimeout(timer);
  });
  const shown = $derived(active || lingering);
  // The side the panel had when the drag began. A drop moves the panel in the same frame the
  // layer hides, and the ghost of the new side must not be torn down in that frame.
  let own = $state<Side>(untrack(() => dock[panel]));
  $effect(() => {
    if (!shown) own = dock[panel];
  });

  // The ghosts of the other sides mount one frame after another, after the first paint of
  // the drag, so no single frame pays for all of them. The candidate side always shows its own.
  let ready = $state<Side[]>([]);
  $effect(() => {
    if (!shown) {
      ready = [];
      return;
    }
    const todo = DOCK_SIDES[panel].filter((side) => side !== untrack(() => own));
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
    const anchor = {
      left: `left:${r.left}px;top:${r.top}px`,
      top: `left:${r.left}px;top:${r.top}px`,
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
  <div class="ghosts" class:hidden={!active} aria-hidden="true" inert use:toBody>
    {#each DOCK_SIDES[panel] as side (side)}
      {@const size = geometry.thickness(side)}
      {@const on = drag?.candidate === side}
      <div class="slot" class:on style={slotStyle(region, side, size, on)}>
        {#if side !== own && (on || ready.includes(side))}
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
  .ghosts.hidden {
    visibility: hidden;
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
    pointer-events: none;
    user-select: none;
  }
  .frame {
    position: absolute;
    inset: 0;
    box-sizing: border-box;
    border: 2px solid var(--accent);
    border-radius: 6px;
    background: rgba(var(--accent-rgb), 0.08);
  }
</style>
