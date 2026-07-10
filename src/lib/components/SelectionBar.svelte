<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { session, LABELS } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import Flag from "@lucide/svelte/icons/flag";
  import Ban from "@lucide/svelte/icons/ban";
  import FlagOff from "@lucide/svelte/icons/flag-off";
  import X from "@lucide/svelte/icons/x";

  const n = $derived(session.selectedIds.size);

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  // Tags applicable to the current media type.
  const scopedTags = $derived(
    tags.all.filter(
      (t) => t.scope === 2 || (catalog.media === "photos" ? t.scope === 0 : t.scope === 1),
    ),
  );

  // Hover-preview state for the star row (0 = not hovering).
  let hovered = $state(0);

  // These act on session.targets(), which is the current selection when one exists.
  const rate = (r: number) => void session.rate(r);
  const flag = (f: number) => void session.flag(f);
  const label = (l: string) => void session.label(l);
  const toggleTag = (id: number) => void session.toggleTag(id);
</script>

<div class="selbar">
  <span class="head">
    <strong>{n}</strong> selected
    <button class="clear" title="Clear selection (Esc)" aria-label="Clear selection" onclick={() => session.clearSelection()}>
      <X size={12} />
    </button>
  </span>
  <span class="apply">Apply:</span>

  <div class="group">
    <button class="btn" title="Reject" onclick={() => flag(-1)}><Ban size={15} /></button>
    <button class="btn" title="Pick" onclick={() => flag(1)}><Flag size={15} /></button>
    <button class="btn" title="Unflag" onclick={() => flag(0)}><FlagOff size={15} /></button>
  </div>

  <div class="group stars">
    {#each [1, 2, 3, 4, 5] as star (star)}
      <button
        class="star"
        class:preview={hovered >= star}
        aria-label={`Set ${star} stars`}
        onmouseenter={() => (hovered = star)}
        onmouseleave={() => (hovered = 0)}
        onclick={() => rate(star)}>★</button
      >
    {/each}
    <button class="star zero" aria-label="Clear rating" onclick={() => rate(0)}>0</button>
  </div>

  <div class="group labels">
    {#each LABELS as l (l)}
      <button
        class="dot"
        style="--c: {labelColors[l]}"
        aria-label={`Label ${l}`}
        onclick={() => label(l)}
      ></button>
    {/each}
  </div>

  {#if scopedTags.length > 0}
    <div class="group tagrow">
      {#each scopedTags as tag (tag.id)}
        <button
          class="tagseg"
          style="--c: {tag.color ?? '#888'}"
          onclick={() => toggleTag(tag.id)}
        >
          <span class="tagdot"></span>{tag.name}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .selbar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 12px;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    flex: none;
    font-size: 12px;
    overflow-x: auto;
    white-space: nowrap;
  }

  .head {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--accent);
    flex: none;
  }

  .head strong {
    color: var(--accent);
  }

  .clear {
    display: inline-flex;
    align-items: center;
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    padding: 1px 2px;
    border-radius: 4px;
  }

  .clear:hover {
    color: #fff;
    background: var(--accent-fill);
  }

  .apply {
    color: #8a93a8;
    flex: none;
  }

  .group {
    display: flex;
    align-items: center;
    gap: 3px;
    flex: none;
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 24px;
    border: 1px solid var(--border-strong);
    background: var(--accent-fill);
    color: var(--accent);
    border-radius: 5px;
    cursor: pointer;
  }

  .btn:hover {
    border-color: var(--accent);
    color: #fff;
  }

  .star {
    background: none;
    border: none;
    color: #6a6a72;
    font-size: 16px;
    cursor: pointer;
    padding: 0 1px;
    line-height: 1;
  }

  .star.preview {
    color: #8a8a93;
  }

  .star.zero {
    font-size: 12px;
    color: #8a8a93;
  }

  .dot {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--c);
    border: 2px solid transparent;
    cursor: pointer;
    opacity: 0.7;
  }

  .dot:hover {
    opacity: 1;
    border-color: #fff;
  }

  .tagseg {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border-strong);
    background: var(--accent-fill);
    color: var(--accent);
    padding: 3px 8px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .tagseg:hover {
    border-color: var(--c);
    color: #fff;
  }

  .tagdot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--c);
  }
</style>
