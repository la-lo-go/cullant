<script lang="ts">
  import { session, type GridDensity } from "../stores/session.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import { settings } from "../stores/settings.svelte";
  import { backdropDismiss } from "../backdrop";
  import { keepClamped } from "../popover";
  import DragList from "./DragList.svelte";
  import InfoTip from "./InfoTip.svelte";
  import X from "@lucide/svelte/icons/x";
  import Link from "@lucide/svelte/icons/link";
  import Unlink from "@lucide/svelte/icons/unlink";

  const dismiss = backdropDismiss(() => (session.viewPanelOpen = false));

  // Pairing only exists where RAWs do; without them the choice is meaningless.
  const hasRaws = $derived(catalog.items.some((i) => i.kind === 0));

  const densities: { value: GridDensity; label: string }[] = [
    { value: "small", label: "Small" },
    { value: "medium", label: "Medium" },
    { value: "large", label: "Large" },
  ];

  const offered = $derived(session.groupDims);

  // Dimensions still free to pick at a given level: any not used elsewhere (the
  // level's own current pick stays selectable so the <select> shows it).
  function available(currentKey?: string) {
    return offered.filter((d) => d.key === currentKey || !session.groupBy.includes(d.key));
  }

  const unused = $derived(offered.filter((d) => !session.groupBy.includes(d.key)));

  let panelEl = $state<HTMLDivElement | null>(null);

  // Focus the panel on open. The toolbar toggle blurs its trigger, so without
  // this nothing inside .panel holds focus and the Escape keydown never reaches
  // onPanelKeydown; it would fall through to the global keymap instead.
  $effect(() => {
    panelEl?.focus();
  });

  $effect(() => keepClamped(() => panelEl));

  function onPanelKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") session.viewPanelOpen = false;
  }
</script>

<div class="backdrop" role="presentation" {...dismiss}></div>

<div
  class="panel"
  bind:this={panelEl}
  role="dialog"
  aria-label="View"
  tabindex="-1"
  onkeydown={onPanelKeydown}
>
  <header>
    <span class="title">View</span>
    <button
      class="close"
      aria-label="Close"
      title="Close"
      onclick={() => (session.viewPanelOpen = false)}
    >
      <X size={15} />
    </button>
  </header>

  <section>
    <span class="lbl">Thumbnail size</span>
    <div class="row">
      {#each densities as d (d.value)}
        <button
          class="seg"
          class:active={session.gridDensity === d.value}
          onclick={() => session.setDensity(d.value)}
        >
          {d.label}
        </button>
      {/each}
    </div>
  </section>

  <section>
    <span class="lbl">Thumbnail framing</span>
    <div class="row">
      <button
        class="seg"
        class:active={settings.gridPhotoFit === "fit"}
        aria-pressed={settings.gridPhotoFit === "fit"}
        onclick={() => settings.setGridPhotoFit("fit")}
      >Fit whole photo</button>
      <button
        class="seg"
        class:active={settings.gridPhotoFit === "fill"}
        aria-pressed={settings.gridPhotoFit === "fill"}
        onclick={() => settings.setGridPhotoFit("fill")}
      >Fill cell</button>
    </div>
  </section>

  <section>
    <button class="toggle" class:on={session.showNames} onclick={() => session.toggleShowNames()}>
      <span>Show file names</span>
      <span class="pill"></span>
    </button>
  </section>

  {#if hasRaws}
    <section>
      <span class="lbl">
        RAW + JPEG pairs
        <InfoTip
          title="RAW + JPEG pairs"
          text="Files sharing a name are one photo. Mirror treats the pair as a single photo, so a rating or a delete hits both files. Separate lists and acts on them independently."
          size={12}
        />
      </span>
      <div class="row">
        <button
          class="seg"
          class:active={session.mirrorMode}
          onclick={() => session.setMirrorMode(true)}
        >
          <Link size={13} /><span>Mirror</span>
        </button>
        <button
          class="seg"
          class:active={!session.mirrorMode}
          onclick={() => session.setMirrorMode(false)}
        >
          <Unlink size={13} /><span>Separate</span>
        </button>
      </div>
    </section>
  {/if}

  {#if session.hasBursts}
    <section>
      <span class="lbl">
        Bursts
        <InfoTip
          title="Collapse bursts"
          text="Collapsed, each burst takes one grid cell drawn as a stack, and acting on it acts on every frame under it. Open a stack to step through the frames with , and . Expanded, every frame gets its own cell and carries its position badge."
          size={12}
        />
      </span>
      <button
        class="toggle"
        class:on={settings.collapseBursts}
        onclick={() => settings.setCollapseBursts(!settings.collapseBursts)}
      >
        <span>Collapse bursts</span>
        <span class="pill"></span>
      </button>
    </section>
  {/if}

  {#if offered.length > 0}
  <section>
    <div class="lbl-row">
      <span class="lbl">Group by</span>
      {#if session.groupBy.length > 0}
        <button class="clear" onclick={() => session.clearGroups()}>Clear</button>
      {/if}
    </div>


    {#if session.activeGroupBy.length > 0}
      <DragList
        items={session.activeGroupBy}
        keyOf={(k) => k}
        onMove={(f, t) => session.reorderGroupLevel(session.groupBy.indexOf(session.activeGroupBy[f]), session.groupBy.indexOf(session.activeGroupBy[t]))}
        ariaLabel="Grouping levels"
      >
        {#snippet row(key)}
          <div class="level">
            <select
              class="dimsel"
              value={key}
              onchange={(e) => session.setGroupLevel(session.groupBy.indexOf(key), e.currentTarget.value)}
            >
              {#each available(key) as d (d.key)}
                <option value={d.key}>{d.label}</option>
              {/each}
            </select>
            <button class="icon" title="Remove" onclick={() => session.removeGroupLevel(session.groupBy.indexOf(key))}>
              <X size={14} />
            </button>
          </div>
        {/snippet}
      </DragList>
    {/if}

    {#if unused.length > 0}
      <select
        class="dimsel add"
        value=""
        onchange={(e) => {
          if (e.currentTarget.value) session.addGroupLevel(e.currentTarget.value);
          e.currentTarget.value = "";
        }}
      >
        <option value="">+ Add level…</option>
        {#each unused as d (d.key)}
          <option value={d.key}>{d.label}</option>
        {/each}
      </select>
    {/if}

    {#if session.activeGroupBy.length > 0}
      <button
        class="toggle"
        class:on={session.stickyGroupHeader}
        onclick={() => (session.stickyGroupHeader = !session.stickyGroupHeader)}
      >
        <span>Sticky group header</span>
        <span class="pill"></span>
      </button>
    {/if}
  </section>
  {/if}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }

  .panel {
    outline: none;
    position: absolute;
    top: 100%;
    right: 0;
    z-index: 41;
    margin-top: 4px;
    width: 288px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2 - var(--inset-left) - var(--inset-right));
    /* Fallback bound only — reserves both safe-area insets and room for the
       toolbar/tap-gap; clampToViewport computes the exact cap once mounted.
       dvh (not vh) so mobile browser chrome is excluded. Same formula as
       FiltersPanel, for the same reason. */
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

  @media (hover: hover) {
    .panel button:hover:not(:disabled) {
      background: var(--hover);
    }
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .lbl,
  .lbl-row {
    color: #8a8a93;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .lbl {
    display: inline-flex;
    align-items: center;
    gap: 1px;
  }

  .lbl-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .clear {
    text-transform: none;
    letter-spacing: 0;
    background: none;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: #bbb;
    padding: 2px 8px;
    cursor: pointer;
    font-size: 11px;
    font-family: inherit;
  }

  .hint {
    margin: 0;
    color: #6a6a72;
  }

  .row {
    display: flex;
    gap: 4px;
  }

  .seg {
    flex: 1;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    border: 1px solid transparent;
    background: var(--control);
    color: #bbb;
    padding: 5px 9px;
    border-radius: 3px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .seg.active {
    background: var(--accent-fill);
    color: #fff;
  }

  .level {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  /* Label + switch row (Show file names / Sticky group header). Its own control
     so the panel never shows a device-default checkbox. */
  .toggle {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    background: none;
    border: none;
    color: #ddd;
    padding: 2px 0;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .pill {
    flex: none;
    position: relative;
    width: 34px;
    height: 18px;
    border-radius: 9px;
    background: var(--control);
    transition: background 0.15s ease;
  }

  .pill::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #888;
    transition:
      transform 0.15s ease,
      background 0.15s ease;
  }

  .toggle.on .pill {
    background: var(--accent-fill);
  }

  .toggle.on .pill::after {
    transform: translateX(16px);
    background: #fff;
  }

  /* The global select rule owns the appearance. This class owns layout only. */
  .dimsel {
    flex: 1;
    min-width: 0;
    font-size: 12px;
  }

  .dimsel.add {
    color: #bbb;
  }

  .icon {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    background: var(--control);
    border: 1px solid transparent;
    border-radius: 4px;
    color: #bbb;
    cursor: pointer;
  }

</style>
