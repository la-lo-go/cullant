<script lang="ts">
  import { previewUrl } from "../api";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  const items = $derived(session.surveyItems);

  // Roughly square: as the field narrows the survivors get a bigger share of
  // the screen, which is the reward for eliminating one.
  const cols = $derived(Math.max(1, Math.ceil(Math.sqrt(items.length))));
</script>

<div class="survey">
  <header>
    <span class="count">
      {items.length} in the running
      {#if session.surveyRequested > 0}
        <span class="capped">· first {items.length} of {session.surveyRequested} selected</span>
      {/if}
    </span>
    <span class="hint">← → to move · X to eliminate · P to pick the winner · Esc to leave</span>
    <button class="close" onclick={() => session.closeSurvey()} aria-label="Close survey" title="Close (Esc)">
      <X size={15} />
    </button>
  </header>

  <div class="tiles" style="grid-template-columns: repeat({cols}, minmax(0, 1fr))">
    {#each items as item (item.id)}
      <button
        class="tile"
        class:focused={session.focused?.id === item.id}
        onclick={() => session.focusSurveyItem(item.id)}
      >
        <img src={previewUrl(item)} alt={item.name} draggable="false" />
        <span class="meta">
          {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
          {#if item.flag === 1}<span class="pick"><Check size={12} /></span>{/if}
          {#if item.label}
            <span class="dot" style="background: {labelColors[item.label] ?? '#888'}"></span>
          {/if}
          <span class="name">{item.name}</span>
        </span>
      </button>
    {/each}
  </div>
</div>

<style>
  .survey {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 12px;
    color: #bbb;
  }

  .count {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .capped {
    opacity: 0.55;
  }

  .hint {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.55;
  }

  .close {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 24px;
    background: none;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: #bbb;
    cursor: pointer;
    padding: 0;
  }

  .close:hover {
    color: #fff;
  }

  .tiles {
    flex: 1;
    min-height: 0;
    display: grid;
    gap: 6px;
  }

  .tile {
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: 2px solid transparent;
    border-radius: 6px;
    padding: 0;
    cursor: pointer;
    overflow: hidden;
  }

  .tile.focused {
    border-color: var(--accent);
  }

  img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  .meta {
    position: absolute;
    left: 4px;
    right: 4px;
    bottom: 4px;
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: #eee;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.9);
    pointer-events: none;
  }

  .stars {
    color: #e0c34f;
    flex: none;
  }

  .pick {
    display: inline-flex;
    color: #59b85e;
    flex: none;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.75;
  }
</style>
