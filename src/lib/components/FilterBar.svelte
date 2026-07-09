<script lang="ts">
  import { session, LABELS, type FlagFilter } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";

  // Only show task tags whose scope matches the current media tab.
  // scope: 0=photo, 1=video, 2=both.
  const scopedTags = $derived(
    tags.all.filter(
      (tag) =>
        tag.scope === 2 ||
        (tag.scope === 0 && catalog.media === "photos") ||
        (tag.scope === 1 && catalog.media === "videos"),
    ),
  );

  const flagOptions: { value: FlagFilter; label: string; icon?: typeof Check }[] = [
    { value: "all", label: "All" },
    { value: "pick", label: "Picks", icon: Check },
    { value: "unflagged", label: "Unflagged" },
    { value: "reject", label: "Rejects", icon: X },
  ];

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  function count(value: FlagFilter): number {
    const c = session.counts;
    switch (value) {
      case "pick":
        return c.pick;
      case "reject":
        return c.reject;
      case "unflagged":
        return c.unflagged;
      default:
        return c.total;
    }
  }
</script>

<div class="filterbar">
  <div class="group">
    {#each flagOptions as opt}
      <button
        class="seg"
        class:active={session.flagFilter === opt.value}
        onclick={() => {
          session.flagFilter = opt.value;
          session.clampFocus();
        }}
      >
        {#if opt.icon}<opt.icon size={12} />{/if}
        <span>{opt.label}</span>
        <span class="count">{count(opt.value)}</span>
      </button>
    {/each}
  </div>

  <div class="group stars" title="Minimum rating">
    {#each [1, 2, 3, 4, 5] as star}
      <button
        class="star"
        class:lit={session.minRating >= star}
        onclick={() => {
          session.minRating = session.minRating === star ? 0 : star;
          session.clampFocus();
        }}>★</button
      >
    {/each}
  </div>

  <div class="group labels" title="Color label filter">
    {#each LABELS as label}
      <button
        class="dot"
        class:active={session.labelFilter === label}
        style="--c: {labelColors[label]}"
        aria-label={label}
        onclick={() => {
          session.labelFilter = session.labelFilter === label ? null : label;
          session.clampFocus();
        }}
      ></button>
    {/each}
  </div>

  {#if scopedTags.length > 0}
    <div class="group tagfilter" title="Task tag filter">
      {#each scopedTags as tag}
        <button
          class="tagseg"
          class:active={session.tagFilter === tag.id}
          style="--c: {tag.color ?? '#888'}"
          onclick={() => {
            session.tagFilter = session.tagFilter === tag.id ? null : tag.id;
            session.clampFocus();
          }}
        >
          <span class="tagdot"></span>{tag.name}
        </button>
      {/each}
    </div>
  {/if}

  <span class="spacer"></span>
  {#if session.selectedIds.size > 0}
    <span class="selection">
      <span>{session.selectedIds.size} selected</span>
      <button
        class="clearsel"
        title="Clear selection (Esc)"
        aria-label="Clear selection"
        onclick={() => session.clearSelection()}
      >
        <X size={12} />
      </button>
    </span>
  {/if}
  <span class="showing">{session.filtered.length} shown</span>
</div>

<style>
  .filterbar {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 3px 10px;
    background: #202026;
    border-bottom: 1px solid #2e2e36;
    flex: none;
    font-size: 11px;
  }

  .group {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid transparent;
    background: transparent;
    color: #bbb;
    padding: 2px 7px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 11px;
    font-family: inherit;
  }

  .seg:hover {
    background: #2a2a30;
  }

  .seg.active {
    background: #33333c;
    color: #fff;
    border-color: #44444f;
  }

  .count {
    opacity: 0.55;
  }

  .star {
    background: none;
    border: none;
    color: #4a4a52;
    font-size: 14px;
    cursor: pointer;
    padding: 0 1px;
  }

  .star.lit {
    color: #ffd166;
  }

  .dot {
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background: var(--c);
    border: 2px solid transparent;
    cursor: pointer;
    opacity: 0.5;
  }

  .dot:hover {
    opacity: 0.85;
  }

  .dot.active {
    opacity: 1;
    border-color: #fff;
  }

  .tagseg {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid transparent;
    background: transparent;
    color: #bbb;
    padding: 2px 7px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 11px;
    font-family: inherit;
  }

  .tagseg:hover {
    background: #2a2a30;
  }

  .tagseg.active {
    background: #33333c;
    color: #fff;
    border-color: var(--c);
  }

  .tagdot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--c);
  }

  .spacer {
    flex: 1;
  }

  .showing {
    opacity: 0.6;
  }

  .selection {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: #9db4ff;
  }

  .clearsel {
    display: inline-flex;
    align-items: center;
    background: none;
    border: none;
    color: #9db4ff;
    cursor: pointer;
    padding: 1px 2px;
    border-radius: 4px;
  }

  .clearsel:hover {
    color: #fff;
    background: #2a2a30;
  }
</style>
