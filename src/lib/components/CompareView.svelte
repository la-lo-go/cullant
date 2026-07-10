<script lang="ts">
  import { previewUrl, type ItemLite } from "../api";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import Filmstrip from "./Filmstrip.svelte";
  import X from "@lucide/svelte/icons/x";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import { edgeBounce } from "../anim";

  type Side = "left" | "right";

  let panes = $state<HTMLElement | null>(null);
  // Bounce the panes when stepping past the first/last photo. Track the
  // last-seen bump so switching into this view doesn't replay a stale bounce.
  let lastBump = session.edgeBump.n;
  $effect(() => {
    const b = session.edgeBump;
    if (b.n === lastBump || !panes) return;
    lastBump = b.n;
    edgeBounce(panes, b.dir, "x");
  });

  // 2-up compare. Each pane owns its zoom (ZoomImage `standalone`), so the two
  // photos can be inspected at different magnifications independently.
  //
  // Pinning freezes one pane on a reference photo while the other follows the
  // focus; with nothing pinned it's focused vs the next in filter order.
  let pinnedSide = $state<Side | null>(null);
  let pinnedItem = $state<ItemLite | null>(null);

  // Prefer the live object from the session (rating/flag edits stay visible),
  // but keep rendering the snapshot if the item drops out of the filter.
  const pinnedLive = $derived.by(() => {
    if (!pinnedItem) return null;
    const id = pinnedItem.id;
    return session.filtered.find((i) => i.id === id) ?? pinnedItem;
  });

  const left = $derived(pinnedSide === "left" ? pinnedLive : session.focused);
  const right = $derived.by(() => {
    if (pinnedSide === "right") return pinnedLive;
    if (pinnedSide === "left") return session.focused;
    return session.filtered[session.focusedIndex + 1];
  });

  // Which pane tracks the focused photo (gets the accent caption).
  const focusedSide = $derived<Side>(pinnedSide === "left" ? "right" : "left");

  function togglePin(side: Side, current: ItemLite | null | undefined) {
    if (pinnedSide === side) {
      pinnedSide = null;
      pinnedItem = null;
    } else if (current) {
      // Pinning one pane unpins the other.
      pinnedSide = side;
      pinnedItem = current;
    }
  }

  // Warm the cache one photo ahead of the focus, so arrowing quickly
  // through a burst always finds the next preview already decoded.
  $effect(() => {
    const ahead = session.filtered[session.focusedIndex + 2];
    if (ahead && ahead.kind !== 2) {
      new Image().src = previewUrl(ahead);
    }
  });
</script>

<div class="compare">
  <div class="panes" bind:this={panes}>
    <button class="back" title="Back to grid (Esc)" onclick={() => (view.mode = "grid")}><X size={16} /></button>
    <button
      class="back filmstrip-btn"
      class:active={session.showFilmstrip}
      title="Show/hide filmstrip (F)"
      onclick={() => session.toggleShowFilmstrip()}
    >
      <PanelBottom size={16} />
    </button>
    {#if left}
      <div class="pane" class:pinned={pinnedSide === "left"}>
        <ZoomImage item={left} standalone />
        <button
          class="pin-btn"
          class:active={pinnedSide === "left"}
          title={pinnedSide === "left" ? "Unpin" : "Pin this photo"}
          onclick={(e) => {
            togglePin("left", left);
            e.currentTarget.blur();
          }}
        >
          {#if pinnedSide === "left"}<Pin size={14} fill="currentColor" />{:else}<PinOff size={14} />{/if}
        </button>
        <span class="caption" class:focused-caption={focusedSide === "left"}>{left.name}.{left.ext}</span>
      </div>
    {/if}
    {#if right}
      <div class="pane" class:pinned={pinnedSide === "right"}>
        <ZoomImage item={right} standalone />
        <button
          class="pin-btn"
          class:active={pinnedSide === "right"}
          title={pinnedSide === "right" ? "Unpin" : "Pin this photo"}
          onclick={(e) => {
            togglePin("right", right);
            e.currentTarget.blur();
          }}
        >
          {#if pinnedSide === "right"}<Pin size={14} fill="currentColor" />{:else}<PinOff size={14} />{/if}
        </button>
        <span class="caption" class:focused-caption={focusedSide === "right"}>{right.name}.{right.ext}</span>
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
    /* Anchor for the collapsed filmstrip's floating peek tab. */
    position: relative;
  }

  .panes {
    flex: 1;
    min-height: 0;
    display: flex;
    gap: 2px;
    position: relative;
  }

  /* Portrait / narrow: stack the two panes vertically instead of side by side. */
  @media (max-width: 640px), (max-aspect-ratio: 3 / 4) {
    .panes {
      flex-direction: column;
    }
  }

  .back {
    position: absolute;
    top: 10px;
    right: calc(10px + var(--safe-right));
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

  .filmstrip-btn {
    top: 46px;
  }

  .filmstrip-btn.active {
    background: rgba(var(--accent-rgb), 0.5);
    color: #fff;
  }

  .pane {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }

  .pane.pinned {
    outline: 1px solid rgba(var(--accent-rgb), 0.55);
    outline-offset: -1px;
  }

  .pin-btn {
    position: absolute;
    top: 10px;
    left: 10px;
    z-index: 5;
    width: 28px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 6px;
    padding: 0;
    font-family: inherit;
    background: rgba(0, 0, 0, 0.45);
    color: rgba(255, 255, 255, 0.75);
    cursor: pointer;
  }

  .pin-btn:hover {
    background: rgba(0, 0, 0, 0.7);
    color: #fff;
  }

  /* Only the first pane touches the left screen edge (side-by-side layout);
     keep its overlays clear of a landscape navigation bar / cutout. */
  .pane:first-child .pin-btn,
  .pane:first-child .caption {
    left: calc(10px + var(--safe-left));
  }

  .pin-btn.active {
    color: var(--accent);
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
    outline: 1px solid var(--accent);
  }

  .pane.empty {
    align-items: center;
    justify-content: center;
    opacity: 0.4;
  }
</style>
