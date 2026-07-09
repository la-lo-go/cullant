<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { session, LABELS, type FlagFilter } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import FilterX from "@lucide/svelte/icons/filter-x";

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

  // Only tags that apply to the current media type (photos vs videos).
  const scopedTags = $derived(
    tags.all.filter(
      (t) => t.scope === 2 || (catalog.media === "photos" ? t.scope === 0 : t.scope === 1),
    ),
  );

  // Hover-preview state for the minimum-rating star row (0 = not hovering).
  let hovered = $state(0);

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

<!-- Backdrop closes the panel on an outside click. -->
<div
  class="backdrop"
  role="presentation"
  onclick={() => (session.filtersPanelOpen = false)}
></div>

<div class="panel" role="dialog" aria-label="Filters">
  <header>
    <span class="title">Filters</span>
    <button
      class="clear"
      disabled={!session.hasActiveFilters}
      onclick={() => session.clearFilters()}
    >
      <FilterX size={13} /> Clear
    </button>
  </header>

  <section>
    <span class="lbl">Flag</span>
    <div class="row">
      {#each flagOptions as opt (opt.value)}
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
  </section>

  <section>
    <span class="lbl">Minimum rating</span>
    <div class="row stars">
      {#each [1, 2, 3, 4, 5] as star (star)}
        <button
          class="star"
          class:lit={hovered === 0 && session.minRating >= star}
          class:preview={hovered >= star}
          aria-label={`At least ${star} stars`}
          onmouseenter={() => (hovered = star)}
          onmouseleave={() => (hovered = 0)}
          onclick={() => {
            session.minRating = session.minRating === star ? 0 : star;
            session.clampFocus();
          }}>★</button
        >
      {/each}
    </div>
  </section>

  <section>
    <span class="lbl">Color label</span>
    <div class="row">
      {#each LABELS as label (label)}
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
  </section>

  {#if scopedTags.length > 0}
    <section>
      <span class="lbl">Task tag</span>
      <div class="row wrap">
        {#each scopedTags as tag (tag.id)}
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
    </section>
  {/if}

  <footer>{session.filtered.length} shown</footer>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }

  .panel {
    position: absolute;
    top: 100%;
    left: 8px;
    z-index: 41;
    margin-top: 4px;
    width: 320px;
    max-width: calc(100vw - 16px);
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px 14px;
    background: #232329;
    border: 1px solid #3a3a42;
    border-radius: 10px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    font-size: 12px;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .title {
    font-weight: 600;
    font-size: 13px;
  }

  .clear {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: none;
    border: 1px solid #3a3a42;
    border-radius: 6px;
    color: #bbb;
    padding: 3px 8px;
    cursor: pointer;
    font-size: 11px;
    font-family: inherit;
  }

  .clear:hover:not(:disabled) {
    color: #fff;
    border-color: #55555f;
  }

  .clear:disabled {
    opacity: 0.4;
    cursor: default;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .lbl {
    color: #8a8a93;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .row.wrap {
    flex-wrap: wrap;
  }

  .seg {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    border: 1px solid transparent;
    background: #2a2a30;
    color: #bbb;
    padding: 4px 9px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .seg:hover {
    background: #33333c;
  }

  .seg.active {
    background: #3a3a46;
    color: #fff;
    border-color: #6b8bff;
  }

  .count {
    opacity: 0.55;
  }

  .stars {
    gap: 2px;
  }

  .star {
    background: none;
    border: none;
    color: #4a4a52;
    font-size: 20px;
    cursor: pointer;
    padding: 0 2px;
    line-height: 1;
  }

  .star.lit {
    color: #ffd166;
  }

  .star.preview {
    color: #8a8a93;
  }

  .dot {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--c);
    border: 2px solid transparent;
    cursor: pointer;
    opacity: 0.55;
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
    background: #2a2a30;
    color: #bbb;
    padding: 4px 9px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .tagseg:hover {
    background: #33333c;
  }

  .tagseg.active {
    background: #3a3a46;
    color: #fff;
    border-color: var(--c);
  }

  .tagdot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--c);
  }

  footer {
    color: #8a8a93;
    border-top: 1px solid #33333b;
    padding-top: 8px;
  }
</style>
