<script lang="ts">
  import { videoUrl } from "../api";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import Filmstrip from "./Filmstrip.svelte";
  import MetadataPanel from "./MetadataPanel.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Scissors from "@lucide/svelte/icons/scissors";
  import Info from "@lucide/svelte/icons/info";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";

  const item = $derived(session.focused);
</script>

<div class="viewer">
  {#if item}
    <div class="stage">
      {#if item.kind === 2}
        <!-- svelte-ignore a11y_media_has_caption -->
        <video class="player" src={videoUrl(item)} controls preload="metadata"></video>
      {:else}
        <ZoomImage {item} />
      {/if}
      <button class="back" title="Back to grid (Esc)" onclick={() => (view.mode = "grid")}><X size={16} /></button>
      <button
        class="back info-btn"
        class:active={view.infoOpen}
        title="Camera metadata (I)"
        data-metadata-toggle
        onclick={() => (view.infoOpen = !view.infoOpen)}
      >
        <Info size={16} />
      </button>
      {#if view.infoOpen}
        <MetadataPanel {item} />
      {/if}
      <button
        class="back filmstrip-btn"
        class:active={session.showFilmstrip}
        title="Show/hide filmstrip (F)"
        onclick={() => session.toggleShowFilmstrip()}
      >
        <PanelBottom size={16} />
      </button>
      <div class="info">
        <span class="filename">{item.relPath}</span>
        {#if session.mirrorMode && item.groupSize > 1}
          <span class="chip" class:split={item.decoupled}>
            {#if item.decoupled}<Scissors size={10} /><span>SPLIT</span>{:else}RAW+JPG{/if}
          </span>
        {/if}
        {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
        {#if item.flag === 1}<span class="pick"><Check size={14} /></span>{/if}
        {#if item.flag === -1}<span class="reject"><X size={14} /></span>{/if}
        {#if item.label}<span class="label">{item.label}</span>{/if}
        <span class="pos">{session.focusedIndex + 1} / {session.filtered.length}</span>
      </div>
    </div>
    <Filmstrip items={session.filtered} />
  {:else}
    <div class="empty">No photo selected</div>
  {/if}
</div>

<style>
  .viewer {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .stage {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    position: relative;
  }

  .player {
    flex: 1;
    min-height: 0;
    background: #131316;
    outline: none;
  }

  .info {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    display: flex;
    gap: 12px;
    align-items: center;
    padding: 6px 12px;
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

  .info-btn {
    top: 46px;
  }

  .info-btn.active {
    background: rgba(107, 139, 255, 0.5);
    color: #fff;
  }

  .filmstrip-btn {
    top: 82px;
  }

  .filmstrip-btn.active {
    background: rgba(107, 139, 255, 0.5);
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

  .label {
    opacity: 0.8;
  }

  .pos {
    margin-left: auto;
    opacity: 0.6;
  }

  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0.5;
  }
</style>
