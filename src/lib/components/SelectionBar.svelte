<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { session, LABELS } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Eraser from "@lucide/svelte/icons/eraser";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import { keymap } from "../keyboard/dispatcher.svelte";
  import { formatKey } from "../keyboard/keymap";
  import { formatColorLabel } from "../colorLabels";

  const n = $derived(session.selectedIds.size);
  const surveyKeys = $derived([...keymap.bindings]
    .filter(([, command]) => command === "view.survey")
    .map(([binding]) => formatKey(binding)).join(" / "));

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  const scopedTags = $derived(
    tags.all.filter(
      (t) => t.scope === 2 || (catalog.media === "photos" ? t.scope === 0 : t.scope === 1),
    ),
  );

  let hovered = $state(0);

  // These act on session.targets(), which is the current selection when one exists.
  const toggleTag = (id: number) => void session.toggleTag(id);

  // Selected items' own data (flag/rating/label/tagIds), so a button can
  // honestly reflect "does the WHOLE selection already have this" rather than
  // always looking pre-applied regardless of actual state.
  const selectedItems = $derived(session.filtered.filter((i) => session.selectedIds.has(i.id)));

  // Shared flag / label / tag state of the whole selection, computed in ONE pass
  // and read many times from the markup (per flag button, per label swatch, per
  // tag). `flag`/`label` hold the common value or null when the selection is
  // mixed/empty; `tagIds` is the intersection of every item's tags.
  const summary = $derived.by(() => {
    const items = selectedItems;
    if (items.length === 0)
      return { flag: null as number | null, label: null as string | null, tagIds: new Set<number>() };
    const first = items[0];
    const flag = items.every((i) => i.flag === first.flag) ? first.flag : null;
    const label = items.every((i) => i.label === first.label) ? first.label : null;
    const tagIds = new Set<number>(
      first.tagIds.filter((t) => items.every((i) => i.tagIds.includes(t))),
    );
    return { flag, label, tagIds };
  });

  // Pick/Reject toggle: when the whole selection already has the flag,
  // clicking again clears it to 0 (unflag).
  const flag = (f: number) => void session.flag(summary.flag === f ? 0 : f);

  // Rating shared by the whole selection; 0 when mixed (or nothing selected),
  // so the stars show nothing lit rather than a misleading partial value.
  const commonRating = $derived(
    selectedItems.length > 0 && selectedItems.every((i) => i.rating === selectedItems[0].rating)
      ? selectedItems[0].rating
      : 0,
  );

  // Stars toggle like the pick/reject flags: clicking the rating the whole
  // selection already shares clears it to 0; any other click sets it.
  const rate = (r: number) => void session.rate(r !== 0 && r === commonRating ? 0 : r);

  // Labels go through the explicit set/clear path: the bar already resolved
  // set-vs-clear from the selection-uniform state it displays, so no toggle.
  const label = (l: string) => void session.setLabel(summary.label === l ? null : l);

  // Run a handler, then release focus so a clicked button never swallows the
  // arrow keys used for grid navigation (mirrors the `blurring()` toolbar helper
  // and TouchActionBar's `act()`).
  function act(fn: () => void): (e: MouseEvent) => void {
    return (e) => {
      fn();
      (e.currentTarget as HTMLElement).blur();
    };
  }
</script>

<div class="selbar">
  <span class="head">
    <strong>{n}</strong> selected
    <button class="clear" title="Clear selection (Esc)" aria-label="Clear selection" onclick={act(() => session.clearSelection())}>
      <X size={12} />
    </button>
  </span>
  {#if session.canSurvey}
    <!-- The survey acts on a selection, so this bar is its natural home — the
         top toolbar has no room and this one only exists when there is
         something to survey. -->
    <button class="btn survey" title={`Survey: review these side by side${surveyKeys ? ` (${surveyKeys})` : ""}`} onclick={act(() => session.openSurvey())}>
      <LayoutGrid size={14} /><span>Review selection</span>
    </button>
  {/if}

  <span class="apply">Apply:</span>

  <div class="group">
    <button class="btn reject" class:active-reject={summary.flag === -1} title="Reject (X). Click again to unflag." onclick={act(() => flag(-1))}><X size={15} /></button>
    <button class="btn pick" class:active-pick={summary.flag === 1} title="Pick (P). Click again to unflag." onclick={act(() => flag(1))}><Check size={15} /></button>
  </div>

  <div class="group stars">
    {#each [1, 2, 3, 4, 5] as star (star)}
      <button
        class="star"
        class:on={commonRating >= star}
        class:preview={hovered >= star}
        aria-label={`Set ${star} stars`}
        onmouseenter={() => (hovered = star)}
        onmouseleave={() => (hovered = 0)}
        onclick={act(() => rate(star))}>★</button
      >
    {/each}
    <button class="star zero" aria-label="Clear rating" onclick={act(() => rate(0))}>0</button>
  </div>

  <div class="group labels">
    {#each LABELS as l (l)}
      <button
        class="dot"
        class:active={summary.label === l}
        style="--c: {labelColors[l]}"
        aria-label={`Label ${formatColorLabel(l)}`}
        title={formatColorLabel(l)}
        onclick={act(() => label(l))}
      ></button>
    {/each}
  </div>

  {#if scopedTags.length > 0}
    <div class="group tagrow">
      {#each scopedTags as tag (tag.id)}
        <button
          class="tagseg"
          class:active={summary.tagIds.has(tag.id)}
          style="--c: {tag.color ?? '#888'}"
          onclick={act(() => toggleTag(tag.id))}
        >
          <span class="tagdot"></span>{tag.name}
        </button>
      {/each}
    </div>
  {/if}

  <div class="group">
    <button
      class="btn"
      title="Clear all classification (rating, flag, label, tags)"
      aria-label="Clear all classification"
      onclick={act(() => session.clearClassification())}
    >
      <Eraser size={15} />
    </button>
  </div>
</div>

<style>
  /* Overlaid at the top of the grid area (the parent mounts it inside the
     relatively-positioned .grid-area), NOT in normal flow: the bar mounting or
     unmounting mid-marquee would otherwise push every thumbnail down/up, which
     re-maps the pointer's row hit-testing and oscillates the selection it just
     created. A subtle shadow separates it from the thumbnails it floats over. */
  .selbar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 30;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 12px;
    box-sizing: border-box;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    box-shadow: 0 2px 10px rgba(0, 0, 0, 0.45);
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
    background: var(--control);
    color: #bbb;
    border-radius: 5px;
    cursor: pointer;
  }

  /* Wider than the icon-only buttons, since it names an action rather than
     applying a state. */
  .btn.survey {
    width: auto;
    gap: 5px;
    padding: 0 9px;
    font-size: 12px;
    font-family: inherit;
  }

  .btn:hover:not(.active) {
    border-color: var(--accent);
    color: #fff;
  }

  /* Pick/reject read as a green check / red X even when inactive, matching the
     thumbnail badges and the touch action bar; "active" adds the tinted
     highlight. */
  .btn.pick {
    color: #6be675;
  }

  .btn.reject {
    color: #ff6b6b;
  }

  .btn.active-pick {
    border-color: #6be675;
    background: rgba(107, 230, 117, 0.14);
  }

  .btn.active-reject {
    border-color: #ff6b6b;
    background: rgba(255, 107, 107, 0.14);
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

  /* Lit stars for the selection's common rating; declared before `.preview`
     so the hover preview wins while hovering. Same yellow as TouchActionBar. */
  .star.on {
    color: #e0c34f;
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

  /* Active = the whole selection carries this label; white outline like
     TouchActionBar's `.swatchbtn.active .swatch`. */
  .dot.active {
    opacity: 1;
    border-color: #fff;
  }

  .tagseg {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    border: 1px solid var(--border-strong);
    background: var(--control);
    color: #bbb;
    padding: 3px 8px;
    border-radius: 5px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .tagseg:hover:not(.active) {
    border-color: var(--c);
    color: #fff;
  }

  .tagseg.active {
    background: var(--accent-fill);
    color: #fff;
    border-color: transparent;
  }

  .tagdot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--c);
  }

  @media (pointer: coarse) {
    .selbar {
      display: none;
    }
  }
</style>
