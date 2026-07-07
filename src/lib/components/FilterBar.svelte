<script lang="ts">
  import { session, LABELS, type FlagFilter } from "../stores/session.svelte";

  const flagOptions: { value: FlagFilter; label: string }[] = [
    { value: "all", label: "All" },
    { value: "pick", label: "✔ Picks" },
    { value: "unflagged", label: "Unflagged" },
    { value: "reject", label: "✖ Rejects" },
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
        {opt.label}
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

  <span class="spacer"></span>
  <span class="showing">{session.filtered.length} shown</span>
</div>

<style>
  .filterbar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 6px 12px;
    background: #202026;
    border-bottom: 1px solid #2e2e36;
    flex: none;
    font-size: 12px;
  }

  .group {
    display: flex;
    align-items: center;
    gap: 2px;
  }

  .seg {
    border: 1px solid transparent;
    background: transparent;
    color: #bbb;
    padding: 3px 8px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 12px;
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
    margin-left: 4px;
  }

  .star {
    background: none;
    border: none;
    color: #4a4a52;
    font-size: 15px;
    cursor: pointer;
    padding: 0 1px;
  }

  .star.lit {
    color: #ffd166;
  }

  .dot {
    width: 14px;
    height: 14px;
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

  .spacer {
    flex: 1;
  }

  .showing {
    opacity: 0.6;
  }
</style>
