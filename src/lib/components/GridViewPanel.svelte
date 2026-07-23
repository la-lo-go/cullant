<script lang="ts">
  import { session, type GridDensity } from "../stores/session.svelte";
  import { GROUP_DIMS } from "../gridGroups";
  import DragList from "./DragList.svelte";
  import X from "@lucide/svelte/icons/x";

  const densities: { value: GridDensity; label: string }[] = [
    { value: "small", label: "Small" },
    { value: "medium", label: "Medium" },
    { value: "large", label: "Large" },
  ];

  // Dimensions still free to pick at a given level: any not used elsewhere (the
  // level's own current pick stays selectable so the <select> shows it).
  function available(currentKey?: string) {
    return GROUP_DIMS.filter((d) => d.key === currentKey || !session.groupBy.includes(d.key));
  }

  const unused = $derived(GROUP_DIMS.filter((d) => !session.groupBy.includes(d.key)));

  let panelEl = $state<HTMLDivElement | null>(null);
  $effect(() => {
    panelEl?.focus();
  });

  function onPanelKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") session.viewPanelOpen = false;
  }
</script>

<div class="backdrop" role="presentation" onclick={() => (session.viewPanelOpen = false)}></div>

<div
  class="panel"
  bind:this={panelEl}
  role="dialog"
  aria-label="Grid view"
  tabindex="-1"
  onkeydown={onPanelKeydown}
>
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
    <button class="toggle" class:on={session.showNames} onclick={() => session.toggleShowNames()}>
      <span>Show file names</span>
      <span class="pill"></span>
    </button>
  </section>

  <section>
    <div class="lbl-row">
      <span class="lbl">Group by</span>
      {#if session.groupBy.length > 0}
        <button class="clear" onclick={() => session.clearGroups()}>Clear</button>
      {/if}
    </div>

    {#if session.groupBy.length === 0}
      <p class="hint">Not grouped — one flat grid.</p>
    {/if}

    {#if session.groupBy.length > 0}
      <DragList
        items={session.groupBy}
        keyOf={(k) => k}
        onMove={(f, t) => session.reorderGroupLevel(f, t)}
        ariaLabel="Grouping levels"
      >
        {#snippet row(key, i)}
          <div class="level">
            <select
              class="dimsel"
              value={key}
              onchange={(e) => session.setGroupLevel(i, e.currentTarget.value)}
            >
              {#each available(key) as d (d.key)}
                <option value={d.key}>{d.label}</option>
              {/each}
            </select>
            <button class="icon" title="Remove" onclick={() => session.removeGroupLevel(i)}>
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

    {#if session.groupBy.length > 0}
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
    left: 0;
    z-index: 41;
    margin-top: 4px;
    width: 288px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2 - var(--inset-left) - var(--inset-right));
    max-height: calc(100vh - 80px - var(--inset-bottom));
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

  .clear:hover {
    color: #fff;
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
    border: 1px solid transparent;
    background: var(--control);
    color: #bbb;
    padding: 5px 9px;
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

  /* Look comes from the app-wide :global(select) rule; only layout here. */
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

  .icon:hover {
    border-color: var(--accent);
    color: #fff;
  }
</style>
