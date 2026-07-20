<script lang="ts">
  import { previewUrl, type ItemLite } from "../api";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import { tags } from "../stores/tags.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import Filmstrip from "./Filmstrip.svelte";
  import X from "@lucide/svelte/icons/x";
  import Pin from "@lucide/svelte/icons/pin";
  import PinOff from "@lucide/svelte/icons/pin-off";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Minimize from "@lucide/svelte/icons/minimize";
  import { edgeBounce } from "../anim";

  type Side = "left" | "right";

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

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

  /** Swipe/margin-tap paging on a PINNED pane: moving the shared focus
   *  wouldn't change what a pinned pane shows, so page the pinned photo
   *  itself instead, through the same filtered order. */
  function pageWhilePinned(dir: number) {
    if (!pinnedItem) return;
    const idx = session.filtered.findIndex((i) => i.id === pinnedItem!.id);
    if (idx < 0) return;
    const next = session.filtered[idx + dir];
    if (next) pinnedItem = next;
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
    <button
      class="back"
      title="Back to grid (Esc)"
      onclick={(e) => {
        view.mode = "grid";
        (e.currentTarget as HTMLElement).blur();
      }}><X size={16} /></button
    >
    <button
      class="back fullscreen-btn"
      class:active={view.fullscreen}
      title={view.fullscreen ? "Exit full screen" : "Full screen"}
      onclick={(e) => {
        view.toggleFullscreen();
        (e.currentTarget as HTMLElement).blur();
      }}
    >
      {#if view.fullscreen}<Minimize size={16} />{:else}<Maximize size={16} />{/if}
    </button>
    {#if left}
      <div class="pane" class:pinned={pinnedSide === "left"}>
        <ZoomImage item={left} standalone onPage={pinnedSide === "left" ? pageWhilePinned : undefined} />
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
        {#if !view.fullscreen}
          <span class="caption" class:focused-caption={focusedSide === "left"}>
            <span style:color={left.label ? labelColors[left.label] : null}>{left.name}.{left.ext}</span>
            {#each left.tagIds as tagId (tagId)}
              {@const t = tags.byId.get(tagId)}
              {#if t}
                {@const c = t.color ?? "#888"}
                <span class="tagpill" style="border-color: {c}; background: {c}2e">{t.name}</span>
              {/if}
            {/each}
          </span>
        {/if}
      </div>
    {/if}
    {#if right}
      <div class="pane" class:pinned={pinnedSide === "right"}>
        <ZoomImage item={right} standalone onPage={pinnedSide === "right" ? pageWhilePinned : undefined} />
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
        {#if !view.fullscreen}
          <span class="caption" class:focused-caption={focusedSide === "right"}>
            <span style:color={right.label ? labelColors[right.label] : null}>{right.name}.{right.ext}</span>
            {#each right.tagIds as tagId (tagId)}
              {@const t = tags.byId.get(tagId)}
              {#if t}
                {@const c = t.color ?? "#888"}
                <span class="tagpill" style="border-color: {c}; background: {c}2e">{t.name}</span>
              {/if}
            {/each}
          </span>
        {/if}
      </div>
    {:else}
      <div class="pane empty">End of set</div>
    {/if}
  </div>
  {#if !view.fullscreen}
    <Filmstrip items={session.filtered} />
  {/if}
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

  .fullscreen-btn {
    top: 46px;
  }

  .fullscreen-btn.active {
    background: rgba(var(--accent-rgb), 0.5);
    color: #fff;
  }

  .pane {
    flex: 1;
    min-width: 0;
    /* Flex items default min-height to `auto` (their content's intrinsic
       size), not 0 — without this, stacking the panes vertically (narrow /
       portrait) let each pane's content push it past its 1/2 share of the
       column, spilling the second pane below the viewport uncontained. */
    min-height: 0;
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
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    max-width: calc(100% - 20px);
    font-size: 12px;
    opacity: 0.7;
    background: rgba(0, 0, 0, 0.5);
    padding: 2px 8px;
    border-radius: 4px;
    pointer-events: none;
  }

  .tagpill {
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 4px;
    border: 1px solid;
    /* Border color and a ~18% alpha tint (2e suffix) are set inline from the
       tag's #rrggbb color. */
    color: #eee;
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
