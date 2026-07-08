<script lang="ts">
  import { session } from "../stores/session.svelte";
  import ZoomImage from "./ZoomImage.svelte";
  import Filmstrip from "./Filmstrip.svelte";

  const item = $derived(session.focused);
</script>

<div class="viewer">
  {#if item}
    <div class="stage">
      <ZoomImage {item} />
      <div class="info">
        <span class="filename">{item.relPath}</span>
        {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
        {#if item.flag === 1}<span class="pick">✔</span>{/if}
        {#if item.flag === -1}<span class="reject">✖</span>{/if}
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

  .stars {
    color: #ffd166;
  }

  .pick {
    color: #6be675;
  }

  .reject {
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
