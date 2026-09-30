<script lang="ts">
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import VideoPlayer from "./VideoPlayer.svelte";
  import Filmstrip from "./Filmstrip.svelte";
  import MetadataPanel from "./MetadataPanel.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Layers from "@lucide/svelte/icons/layers";
  import Info from "@lucide/svelte/icons/info";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Minimize from "@lucide/svelte/icons/minimize";
  import { edgeBounce } from "../anim";
  import { previewUrl } from "../api";
  import { tags } from "../stores/tags.svelte";

  const item = $derived(session.focused);

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  // Warm the neighbours so arrowing doesn't wait on a cold decode. Both the
  // window and the dwell used to be small because the pool had no bound: a
  // burst of warm requests would each be decoded in full however stale they got.
  // Duplicates now coalesce and overflow evicts oldest-first, so warming wider
  // and sooner costs nothing when the user keeps moving.
  //
  // The previewReady check is gone with it — it lags by up to 600ms, so it
  // mostly skipped neighbours that were NOT cached. A warm request for one that
  // is cached is served from disk with no pool work anyway.
  const WARM_OFFSETS = [1, -1, 2, -2, 3];
  const WARM_DWELL_MS = 300;
  $effect(() => {
    const idx = session.focusedIndex;
    const timer = setTimeout(() => {
      for (const off of WARM_OFFSETS) {
        const n = session.filtered[idx + off];
        if (n && n.kind !== 2) {
          new Image().src = previewUrl(n);
        }
      }
    }, WARM_DWELL_MS);
    return () => clearTimeout(timer);
  });

  let stage = $state<HTMLElement | null>(null);
  // Bounce the image when the user tries to step past the first/last photo.
  // Track the last-seen bump so switching *into* this view (which remounts the
  // component with an already-nonzero counter) doesn't replay a stale bounce.
  let lastBump = session.edgeBump.n;
  $effect(() => {
    const b = session.edgeBump;
    if (b.n === lastBump || !stage) return;
    lastBump = b.n;
    edgeBounce(stage, b.dir, "x");
  });
</script>

<div class="viewer">
  {#if item}
    <div class="stage" bind:this={stage}>
      {#if item.kind === 2}
        {#key item.id}
          <VideoPlayer {item} />
        {/key}
      {:else}
        <ZoomImage {item} />
      {/if}
      <button
        class="back"
        title="Back to grid (Esc)"
        onclick={(e) => {
          view.mode = "grid";
          (e.currentTarget as HTMLElement).blur();
        }}><X size={16} /></button
      >
      <button
        class="back info-btn"
        class:active={view.infoOpen}
        title="Camera metadata (I)"
        data-metadata-toggle
        onclick={(e) => {
          view.infoOpen = !view.infoOpen;
          (e.currentTarget as HTMLElement).blur();
        }}
      >
        <Info size={16} />
      </button>
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
      {#if view.infoOpen}
        <MetadataPanel {item} />
      {/if}
      {#if !view.fullscreen}
        <div class="info">
          <span class="filename" style:color={item.label ? labelColors[item.label] : null}>{item.relPath}</span>
          {#if session.mirrorMode && item.groupSize > 1}
            <span class="chip" class:split={item.decoupled}>
              {#if item.decoupled}<Scissors size={10} /><span>SPLIT</span>{:else}RAW+JPG{/if}
            </span>
          {/if}
          {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
          {#if item.flag === 1}<span class="pick"><Check size={14} /></span>{/if}
          {#if item.flag === -1}<span class="reject"><X size={14} /></span>{/if}
          {#each item.tagIds as tagId (tagId)}
            {@const t = tags.byId.get(tagId)}
            {#if t}
              {@const c = t.color ?? "#888"}
              <span class="tagpill" style="border-color: {c}; background: {c}2e">{t.name}</span>
            {/if}
          {/each}
          {#if session.focusedBurst}
            <span class="burst" title="Shot {session.focusedBurst.position} of a burst of
{session.focusedBurst.total} · step with , and .">
              <Layers size={11} />
              {session.focusedBurst.position}/{session.focusedBurst.total}
            </span>
          {/if}
          <span class="pos">{session.focusedIndex + 1} / {session.filtered.length}</span>
        </div>
      {/if}
    </div>
  {:else}
    <div class="empty" role="status">
      <p>No photo selected. The previous photo may no longer be available.</p>
      <button onclick={(e) => { view.mode = "grid"; e.currentTarget.blur(); }}>
        <LayoutGrid size={16} /> Back to grid
      </button>
    </div>
  {/if}
  {#if !view.fullscreen}
    <Filmstrip items={session.filtered} />
  {/if}
</div>

<style>
  .viewer {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    /* Anchor for the collapsed filmstrip's floating peek tab. */
    position: relative;
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }

  .info {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 4px 12px;
    align-items: center;
    padding: 6px calc(12px + var(--safe-right)) 6px calc(12px + var(--safe-left));
    font-size: 12px;
    background: linear-gradient(transparent, rgba(0, 0, 0, 0.6));
    pointer-events: none;
  }

  .filename {
    opacity: 0.8;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 10px;
    font-weight: 600;
    padding: 1px 5px;
    border-radius: 4px;
    background: rgba(0, 0, 0, 0.55);
    color: #8fd0ff;
  }

  .chip.split {
    color: #ffb86b;
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

  .info-btn {
    top: 46px;
  }

  .info-btn.active {
    background: rgba(var(--accent-rgb), 0.5);
    color: #fff;
  }

  .fullscreen-btn {
    top: 82px;
  }

  .fullscreen-btn.active {
    background: rgba(var(--accent-rgb), 0.5);
    color: #fff;
  }

  .stars {
    color: #ffd166;
  }

  .pick {
    display: inline-flex;
    align-items: center;
    color: #6be675;
  }

  .reject {
    display: inline-flex;
    align-items: center;
    color: #ff6b6b;
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

  .pos {
    margin-left: auto;
    opacity: 0.6;
  }

  .burst {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    padding: 1px 7px;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    opacity: 0.8;
  }

  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-direction: column;
    gap: 12px;
    padding: 20px;
    text-align: center;
    color: #888;
  }

  .empty button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 10px 16px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
</style>
