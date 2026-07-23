<script lang="ts">
  import { session, type GridDensity } from "../stores/session.svelte";
  import { GROUP_DIMS, groupDim } from "../gridGroups";
  import Plus from "@lucide/svelte/icons/plus";
  import X from "@lucide/svelte/icons/x";
  import ChevronUp from "@lucide/svelte/icons/chevron-up";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

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
    <div class="lbl-row">
      <span class="lbl">Group by</span>
      {#if session.groupBy.length > 0}
        <button class="clear" onclick={() => session.clearGroups()}>Clear</button>
      {/if}
    </div>

    {#if session.groupBy.length === 0}
      <p class="hint">Not grouped — one flat grid.</p>
    {/if}

    {#each session.groupBy as key, i (key)}
      <div class="level">
        <span class="depth">{i + 1}</span>
        <select
          class="dimsel"
          value={key}
          onchange={(e) => session.setGroupLevel(i, e.currentTarget.value)}
        >
          {#each available(key) as d (d.key)}
            <option value={d.key}>{d.label}</option>
          {/each}
        </select>
        <button
          class="icon"
          title="Move up"
          disabled={i === 0}
          onclick={() => session.moveGroupLevel(i, -1)}
        >
          <ChevronUp size={14} />
        </button>
        <button
          class="icon"
          title="Move down"
          disabled={i === session.groupBy.length - 1}
          onclick={() => session.moveGroupLevel(i, 1)}
        >
          <ChevronDown size={14} />
        </button>
        <button class="icon" title="Remove" onclick={() => session.removeGroupLevel(i)}>
          <X size={14} />
        </button>
      </div>
    {/each}

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

  .depth {
    width: 16px;
    height: 16px;
    flex: none;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: var(--control);
    color: #8a8a93;
    font-size: 10px;
  }

  .dimsel {
    flex: 1;
    min-width: 0;
    background: var(--control);
    color: #ddd;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    padding: 5px 8px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
  }

  .dimsel:focus {
    outline: none;
    border-color: var(--accent);
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
