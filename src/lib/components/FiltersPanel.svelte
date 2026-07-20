<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import {
    session,
    LABELS,
    type FlagFilter,
    type TypeFilter,
    type OrientationFilter,
  } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import { view } from "../stores/view.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import FilterX from "@lucide/svelte/icons/filter-x";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";

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

  // --- Present-value facets, derived from the loaded catalog. Filtering is
  // entirely client-side over catalog.items, so the same in-memory rows tell us
  // which values the project actually contains: each section offers only those,
  // and hides completely when every present file shares one value (nothing to
  // discriminate). No backend facet query is needed. ---

  // Which RAW/JPEG composition categories occur among the current photos.
  const typePresence = $derived.by(() => {
    let raw = false;
    let jpeg = false;
    let pair = false;
    for (const i of catalog.items) {
      const isPair = i.groupSize > 1 && !i.decoupled;
      if (isPair) pair = true;
      else if (i.kind === 0) raw = true;
      else if (i.kind === 1) jpeg = true;
    }
    return { raw, jpeg, pair };
  });

  const typeOptions = $derived(
    (
      [
        { value: "all", label: "All", show: true },
        { value: "raw", label: "RAW", show: typePresence.raw },
        { value: "jpeg", label: "JPEG", show: typePresence.jpeg },
        { value: "rawjpeg", label: "RAW+JPEG", show: typePresence.pair },
      ] as { value: TypeFilter; label: string; show: boolean }[]
    ).filter((o) => o.show),
  );

  // File type only discriminates when at least two composition categories exist.
  const showTypeSection = $derived(
    catalog.media === "photos" &&
      [typePresence.raw, typePresence.jpeg, typePresence.pair].filter(Boolean).length >= 2,
  );

  // Distinct extensions present in the current media tab (lowercased, sorted).
  const presentExts = $derived.by(() => {
    const s = new Set<string>();
    for (const i of catalog.items) if (i.ext) s.add(i.ext.toLowerCase());
    return [...s].sort();
  });

  // Displayed-aspect orientations that occur (mirrors the filter's rotation
  // handling: EXIF orientation 5-8 means the shown image is turned 90°).
  const presentOrientations = $derived.by(() => {
    const s = new Set<OrientationFilter>();
    for (const i of catalog.items) {
      if (i.width == null || i.height == null) continue;
      const rotated = i.orientation != null && i.orientation >= 5 && i.orientation <= 8;
      const w = rotated ? i.height : i.width;
      const h = rotated ? i.width : i.height;
      if (h > w) s.add("portrait");
      else if (w > h) s.add("landscape");
      else s.add("square");
    }
    return s;
  });

  const orientationOptions = $derived(
    (
      [
        { value: "all", label: "All" },
        { value: "portrait", label: "Portrait" },
        { value: "landscape", label: "Landscape" },
        { value: "square", label: "Square" },
      ] as { value: OrientationFilter; label: string }[]
    ).filter((o) => o.value === "all" || presentOrientations.has(o.value)),
  );

  // Always all 5 — a filter option must stay offered even when nothing in the
  // project currently carries that rating (unlike orientation/label below,
  // which intentionally hide values nothing present has).
  const RATING_STARS = [1, 2, 3, 4, 5];

  // Color labels actually applied somewhere in the project.
  const presentLabels = $derived.by(() => {
    const s = new Set<string>();
    for (const i of catalog.items) if (i.label) s.add(i.label);
    return LABELS.filter((l) => s.has(l));
  });

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

  let panelEl = $state<HTMLDivElement | null>(null);

  // Keeps the panel fully on-screen regardless of where the toolbar button
  // sits (it can be anywhere horizontally once the toolbar wraps on mobile).
  // First cancel any previous offset, then push left if overflowing the
  // right edge, then push right if that pushed it past the left edge. The
  // panel's max-width already guarantees it fits within the margins.
  function clampToViewport() {
    if (!panelEl) return;
    panelEl.style.transform = "";
    const rect = panelEl.getBoundingClientRect();
    const margin = 8;
    let dx = 0;
    if (rect.right > window.innerWidth - margin) dx = window.innerWidth - margin - rect.right;
    if (rect.left + dx < margin) dx = margin - rect.left;
    if (dx) panelEl.style.transform = `translateX(${dx}px)`;
  }

  $effect(() => {
    clampToViewport();
    window.addEventListener("resize", clampToViewport);
    return () => window.removeEventListener("resize", clampToViewport);
  });
</script>

<!-- Backdrop closes the panel on an outside click. -->
<div
  class="backdrop"
  role="presentation"
  onclick={() => (session.filtersPanelOpen = false)}
></div>

<div class="panel" bind:this={panelEl} role="dialog" aria-label="Sort & filter">
  <header>
    <span class="title">Sort & Filter</span>
    <button
      class="clear"
      disabled={!session.hasActiveFilters}
      onclick={() => session.clearFilters()}
    >
      <FilterX size={13} /> Clear
    </button>
  </header>

  {#if view.mode === "grid"}
    <section>
      <span class="lbl">Sort</span>
      <div class="row">
        <button
          class="seg"
          class:active={catalog.sort === "capture"}
          title="Sort by capture time (click again to reverse)"
          onclick={() => void catalog.setSort("capture")}
        >
          <span>Date</span>
          {#if catalog.sort === "capture"}
            {#if catalog.sortDesc}<ArrowDown size={12} />{:else}<ArrowUp size={12} />{/if}
          {/if}
        </button>
        <button
          class="seg"
          class:active={catalog.sort === "name"}
          title="Sort by name (click again to reverse)"
          onclick={() => void catalog.setSort("name")}
        >
          <span>Name</span>
          {#if catalog.sort === "name"}
            {#if catalog.sortDesc}<ArrowDown size={12} />{:else}<ArrowUp size={12} />{/if}
          {/if}
        </button>
      </div>
    </section>
  {/if}

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
      {#each RATING_STARS as star (star)}
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

  {#if presentLabels.length > 0}
    <section>
      <span class="lbl">Color label</span>
      <div class="row">
        {#each presentLabels as label (label)}
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
  {/if}

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

  {#if showTypeSection}
    <section>
      <span class="lbl">File type</span>
      <div class="row">
        {#each typeOptions as opt (opt.value)}
          <button
            class="seg"
            class:active={session.typeFilter === opt.value}
            onclick={() => {
              session.typeFilter = opt.value;
              session.clampFocus();
            }}
          >
            <span>{opt.label}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if presentExts.length > 1}
    <section>
      <span class="lbl">Extension</span>
      <div class="row wrap">
        <button
          class="seg"
          class:active={session.extFilter === null}
          onclick={() => {
            session.extFilter = null;
            session.clampFocus();
          }}
        >
          <span>All</span>
        </button>
        {#each presentExts as ext (ext)}
          <button
            class="seg"
            class:active={session.extFilter === ext}
            onclick={() => {
              session.extFilter = session.extFilter === ext ? null : ext;
              session.clampFocus();
            }}
          >
            <span>{ext.toUpperCase()}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if orientationOptions.length > 1}
    <section>
      <span class="lbl">Orientation</span>
      <div class="row">
        {#each orientationOptions as opt (opt.value)}
          <button
            class="seg"
            class:active={session.orientationFilter === opt.value}
            onclick={() => {
              session.orientationFilter = opt.value;
              session.clampFocus();
            }}
          >
            <span>{opt.label}</span>
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
    /* Baseline anchor; JS (clampToViewport) shifts the panel via transform
       when it would overflow either edge, since the toolbar button's
       horizontal position varies (it can wrap anywhere on mobile). */
    left: 0;
    right: auto;
    z-index: 41;
    margin-top: 4px;
    width: 320px;
    max-width: calc(100vw - 16px);
    /* Cap height to the viewport and scroll internally if the sections are
       tall (e.g. many tags/extensions on a short phone screen). */
    max-height: calc(100vh - 60px);
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px 14px;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
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
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: #bbb;
    padding: 3px 8px;
    cursor: pointer;
    font-size: 11px;
    font-family: inherit;
  }

  .clear:hover:not(:disabled) {
    color: #fff;
    border-color: var(--border-strong);
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
    background: var(--control);
    color: #bbb;
    padding: 4px 9px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .seg:hover:not(.active) {
    border-color: var(--accent);
  }

  .seg.active {
    background: var(--accent-fill);
    color: #fff;
    border-color: transparent;
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
    background: var(--control);
    color: #bbb;
    padding: 4px 9px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .tagseg:hover:not(.active) {
    border-color: var(--c);
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

  footer {
    color: #8a8a93;
    border-top: 1px solid var(--border);
    padding-top: 8px;
  }
</style>
