<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { backdropDismiss } from "../backdrop";
  import { keepClamped } from "../popover";
  import {
    session,
    LABELS,
    type FlagFilter,
    type TypeFilter,
    type OrientationFilter,
  } from "../stores/session.svelte";
  import { tags } from "../stores/tags.svelte";
  import {
    APERTURE_BUCKETS,
    FOCAL_BUCKETS,
    ISO_BUCKETS,
    SHUTTER_BUCKETS,
    apertureBucket,
    focalBucket,
    isoBucket,
    mergeSpellings,
    shutterBucket,
  } from "../metadataFacets";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Flag from "@lucide/svelte/icons/flag";
  import ShieldCheck from "@lucide/svelte/icons/shield-check";
  import FilterX from "@lucide/svelte/icons/filter-x";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";

  // The four plain flag states first, then the two combinations.
  const flagOptions: { value: FlagFilter; label: string; icon?: typeof Check }[] = [
    { value: "all", label: "All" },
    { value: "pick", label: "Picks", icon: Check },
    { value: "unflagged", label: "Unflagged" },
    { value: "reject", label: "Rejects", icon: X },
    { value: "anyflag", label: "Any flag", icon: Flag },
    { value: "notrejected", label: "Not rejected", icon: ShieldCheck },
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
  // discriminate). No backend facet query is needed. All four facets are filled
  // in ONE pass over catalog.items — a keystroke-rate remap must not re-scan the
  // whole catalog four times. ---
  const facets = $derived.by(() => {
    let raw = false;
    let jpeg = false;
    let pair = false;
    const exts = new Set<string>();
    const orientations = new Set<OrientationFilter>();
    const labels = new Set<string>();
    const cameras = new Map<string, number>();
    const lenses = new Map<string, number>();
    const isoKeys = new Set<string>();
    const apertureKeys = new Set<string>();
    const focalKeys = new Set<string>();
    const shutterKeys = new Set<string>();
    for (const i of catalog.items) {
      const isPair = i.groupSize > 1 && !i.decoupled;
      if (isPair) pair = true;
      else if (i.kind === 0) raw = true;
      else if (i.kind === 1) jpeg = true;

      if (i.ext) exts.add(i.ext.toLowerCase());

      // Photographic-settings facets. camera/lens count each spelling, because
      // one body can be written in several capitalisations and the menu must
      // offer it once (see mergeSpellings); the numeric three are collapsed to a
      // bucket so auto-mode variety can't flood the menu.
      if (i.camera) cameras.set(i.camera, (cameras.get(i.camera) ?? 0) + 1);
      if (i.lens) lenses.set(i.lens, (lenses.get(i.lens) ?? 0) + 1);
      const ib = isoBucket(i.iso);
      if (ib) isoKeys.add(ib.key);
      const ab = apertureBucket(i.fNumber);
      if (ab) apertureKeys.add(ab.key);
      const fb = focalBucket(i.focalLength);
      if (fb) focalKeys.add(fb.key);
      const sb = shutterBucket(i.exposureTime);
      if (sb) shutterKeys.add(sb.key);

      if (i.width != null && i.height != null) {
        // Mirrors the filter's rotation handling: EXIF orientation 5-8 means the
        // shown image is turned 90°, so use the displayed (swapped) dimensions.
        const rotated = i.orientation != null && i.orientation >= 5 && i.orientation <= 8;
        const w = rotated ? i.height : i.width;
        const h = rotated ? i.width : i.height;
        if (h > w) orientations.add("portrait");
        else if (w > h) orientations.add("landscape");
        else orientations.add("square");
      }

      if (i.label) labels.add(i.label);
    }
    return {
      typePresence: { raw, jpeg, pair },
      exts,
      orientations,
      labels,
      cameras,
      lenses,
      isoKeys,
      apertureKeys,
      focalKeys,
      shutterKeys,
    };
  });

  // Which RAW/JPEG composition categories occur among the current photos.
  const typePresence = $derived(facets.typePresence);

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
  const presentExts = $derived([...facets.exts].sort());

  // Displayed-aspect orientations that occur.
  const presentOrientations = $derived(facets.orientations);

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
  const presentLabels = $derived(LABELS.filter((l) => facets.labels.has(l)));

  // Photographic-settings facets present in the current photos. camera/lens show
  // one dropdown entry per case-insensitive name; the numeric three keep the
  // canonical low-to-high bucket order (only present buckets survive).
  const presentCameras = $derived(mergeSpellings(facets.cameras));
  const presentLenses = $derived(mergeSpellings(facets.lenses));
  const presentIsoBuckets = $derived(ISO_BUCKETS.filter((b) => facets.isoKeys.has(b.key)));
  const presentApertureBuckets = $derived(
    APERTURE_BUCKETS.filter((b) => facets.apertureKeys.has(b.key)),
  );
  const presentFocalBuckets = $derived(FOCAL_BUCKETS.filter((b) => facets.focalKeys.has(b.key)));
  const presentShutterBuckets = $derived(
    SHUTTER_BUCKETS.filter((b) => facets.shutterKeys.has(b.key)),
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
      case "anyflag":
        return c.pick + c.reject;
      case "notrejected":
        return c.total - c.reject;
      default:
        return c.total;
    }
  }

  // A control that keeps DOM focus after a click swallows the arrow keys: the
  // panel's keydown handler stops them before the grid sees them, and a focused
  // <select> even consumes them itself. So release focus after a pointer pick,
  // but not after a keyboard one — a user who tabs into the panel keeps place.
  let pointerPick = false;

  function releaseAfterPointerPick(e: Event) {
    if (!pointerPick) return;
    pointerPick = false;
    (e.currentTarget as HTMLElement).blur();
  }

  // Same rule for the chips, delegated once instead of per button. A real click
  // reports detail > 0; keyboard activation reports 0.
  function releaseChipFocus(e: MouseEvent) {
    if (e.detail === 0) return;
    (e.target as HTMLElement | null)?.closest("button")?.blur();
  }

  let panelEl = $state<HTMLDivElement | null>(null);

  const dismiss = backdropDismiss(() => (session.filtersPanelOpen = false));

  // Focus the panel on open. The toolbar toggle blurs its trigger, so without
  // this nothing inside .panel holds focus and the Escape keydown never reaches
  // onPanelKeydown; it would fall through to the global keymap instead.
  $effect(() => {
    panelEl?.focus();
  });

  $effect(() => keepClamped(() => panelEl));

  function onPanelKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") session.filtersPanelOpen = false;
  }
</script>

<!-- Backdrop closes the panel on an outside click. -->
<div class="backdrop" role="presentation" {...dismiss}></div>

<div
  class="panel"
  bind:this={panelEl}
  role="dialog"
  aria-label="Sort & filter"
  tabindex="-1"
  onkeydown={onPanelKeydown}
  onclick={releaseChipFocus}
>
  <header>
    <span class="title">Sort & Filter</span>
    <div class="header-actions">
      <button
        class="clear"
        disabled={!session.hasActiveFilters}
        onclick={() => session.clearFilters()}
      >
        <FilterX size={13} /> Clear
      </button>
      <button
        class="close"
        aria-label="Close"
        title="Close"
        onclick={() => (session.filtersPanelOpen = false)}
      >
        <X size={15} />
      </button>
    </div>
  </header>

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
      <button
        class="seg"
        class:active={catalog.sort === "size"}
        title="Sort by file size (click again to reverse)"
        onclick={() => void catalog.setSort("size")}
      >
        <span>Size</span>
        {#if catalog.sort === "size"}
          {#if catalog.sortDesc}<ArrowDown size={12} />{:else}<ArrowUp size={12} />{/if}
        {/if}
      </button>
    </div>
  </section>

  <section>
    <span class="lbl">File name</span>
    <input
      class="namefield"
      type="text"
      placeholder="Any, or /regex/"
      title="Substring match, or a regular expression between slashes"
      spellcheck="false"
      autocomplete="off"
      bind:value={session.nameFilter}
      oninput={() => session.clampFocus()}
    />
  </section>

  <section>
    <span class="lbl">Flag</span>
    <!-- Six chips never fit one line, so this row wraps like the tag rows. -->
    <div class="row wrap">
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

  {#if presentCameras.length > 1}
    <section>
      <span class="lbl">Camera</span>
      <select
        class="metaselect"
        value={session.cameraFilter ?? ""}
        onpointerdown={() => (pointerPick = true)}
        onkeydown={() => (pointerPick = false)}
        onchange={(e) => {
          session.cameraFilter = e.currentTarget.value || null;
          session.clampFocus();
          releaseAfterPointerPick(e);
        }}
      >
        <option value="">Any</option>
        {#each presentCameras as cam (cam)}
          <option value={cam}>{cam}</option>
        {/each}
      </select>
    </section>
  {/if}

  {#if presentLenses.length > 1}
    <section>
      <span class="lbl">Lens</span>
      <select
        class="metaselect"
        value={session.lensFilter ?? ""}
        onpointerdown={() => (pointerPick = true)}
        onkeydown={() => (pointerPick = false)}
        onchange={(e) => {
          session.lensFilter = e.currentTarget.value || null;
          session.clampFocus();
          releaseAfterPointerPick(e);
        }}
      >
        <option value="">Any</option>
        {#each presentLenses as lens (lens)}
          <option value={lens}>{lens}</option>
        {/each}
      </select>
    </section>
  {/if}

  {#if presentIsoBuckets.length > 1}
    <section>
      <span class="lbl">ISO</span>
      <div class="row wrap">
        <button
          class="seg"
          class:active={session.isoFilter === null}
          onclick={() => {
            session.isoFilter = null;
            session.clampFocus();
          }}
        >
          <span>All</span>
        </button>
        {#each presentIsoBuckets as b (b.key)}
          <button
            class="seg"
            class:active={session.isoFilter === b.key}
            onclick={() => {
              session.isoFilter = session.isoFilter === b.key ? null : b.key;
              session.clampFocus();
            }}
          >
            <span>{b.label}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if presentApertureBuckets.length > 1}
    <section>
      <span class="lbl">Aperture</span>
      <div class="row wrap">
        <button
          class="seg"
          class:active={session.apertureFilter === null}
          onclick={() => {
            session.apertureFilter = null;
            session.clampFocus();
          }}
        >
          <span>All</span>
        </button>
        {#each presentApertureBuckets as b (b.key)}
          <button
            class="seg"
            class:active={session.apertureFilter === b.key}
            onclick={() => {
              session.apertureFilter = session.apertureFilter === b.key ? null : b.key;
              session.clampFocus();
            }}
          >
            <span>{b.label}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if presentFocalBuckets.length > 1}
    <section>
      <span class="lbl">Focal length</span>
      <div class="row wrap">
        <button
          class="seg"
          class:active={session.focalFilter === null}
          onclick={() => {
            session.focalFilter = null;
            session.clampFocus();
          }}
        >
          <span>All</span>
        </button>
        {#each presentFocalBuckets as b (b.key)}
          <button
            class="seg"
            class:active={session.focalFilter === b.key}
            onclick={() => {
              session.focalFilter = session.focalFilter === b.key ? null : b.key;
              session.clampFocus();
            }}
          >
            <span>{b.label}</span>
          </button>
        {/each}
      </div>
    </section>
  {/if}

  {#if presentShutterBuckets.length > 1}
    <section>
      <span class="lbl">Shutter speed</span>
      <div class="row wrap">
        <button
          class="seg"
          class:active={session.shutterFilter === null}
          onclick={() => {
            session.shutterFilter = null;
            session.clampFocus();
          }}
        >
          <span>All</span>
        </button>
        {#each presentShutterBuckets as b (b.key)}
          <button
            class="seg"
            class:active={session.shutterFilter === b.key}
            onclick={() => {
              session.shutterFilter = session.shutterFilter === b.key ? null : b.key;
              session.clampFocus();
            }}
          >
            <span>{b.label}</span>
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
    /* The panel takes focus on open (so Escape works); it is not a text field,
       so suppress the focus ring the browser would draw around it. */
    outline: none;
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
    /* Same gutter as the centered dialogs (shared token, widened on narrow
       portrait phones) plus the side safe-area insets, so the dropdown never
       hugs the screen edges; clampToViewport enforces the same margin. */
    max-width: calc(
      100vw - var(--dialog-edge-margin) * 2 - var(--inset-left) - var(--inset-right)
    );
    /* Cap height to the viewport and scroll internally if the sections are tall
       (e.g. many facets on a short phone screen). Fallback bound only — reserves
       both safe-area insets and room for the toolbar/tap-gap; clampToViewport
       computes the exact cap once mounted. dvh (not vh) so mobile browser chrome
       is excluded. */
    max-height: calc(100dvh - var(--inset-top) - var(--inset-bottom) - 96px);
    overflow-y: auto;
    scrollbar-width: none;
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

  .panel::-webkit-scrollbar {
    display: none;
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

  .header-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .close {
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
    border-color: var(--border-strong);
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
    /* Wrap by default: a section's chips must fall onto a second line rather
       than squeeze onto one and overflow the fixed-width panel. `.wrap` is kept
       as an explicit marker on the deliberately-growable groups. */
    flex-wrap: wrap;
  }

  .row.wrap {
    flex-wrap: wrap;
  }

  /* Look comes from the app-wide :global(select) rule; only layout here. */
  .metaselect {
    width: 100%;
    font-size: 12px;
  }

  .namefield {
    width: 100%;
    box-sizing: border-box;
    background: var(--control);
    border: 1px solid transparent;
    border-radius: 3px;
    color: #eee;
    padding: 5px 8px;
    font-size: 12px;
    font-family: inherit;
  }

  .namefield::placeholder {
    color: #6a6a72;
  }

  .namefield:focus {
    outline: none;
    border-color: var(--accent);
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
