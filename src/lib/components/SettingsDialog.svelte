<script lang="ts">
  import {
    settings,
    PREVIEW_QUALITY_CHOICES,
    PREVIEW_QUALITY_LABELS,
  } from "../stores/settings.svelte";
  import {
    FILMSTRIP_BADGES,
    GROUPS,
    SETTINGS,
    anyModified,
    groupModified,
    matches,
    resetAll,
    resetGroup,
    type GroupId,
    type Setting,
    type SettingGroup,
  } from "../settingsSchema";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import { api } from "../api";
  import { backdropDismiss } from "../backdrop";
  import { keepClamped } from "../popover";
  import DragList from "./DragList.svelte";
  import Info from "@lucide/svelte/icons/info";
  import Search from "@lucide/svelte/icons/search";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Tag from "@lucide/svelte/icons/tag";
  import Heart from "@lucide/svelte/icons/heart";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import X from "@lucide/svelte/icons/x";

  let {
    onclose,
    onshowkeybindings,
    onshowtags,
    onshowsupport,
  }: {
    onclose: () => void;
    /** Open the keyboard-shortcuts dialog (owned by the page). */
    onshowkeybindings: () => void;
    /** Open the task-tag editor (owned by the page). */
    onshowtags: () => void;
    /** Open the support dialog (owned by the page). */
    onshowsupport: () => void;
  } = $props();

  // Touch platform (Android) reaches the manual rescan via pull-to-refresh;
  // desktop uses the title-bar menu. The auto-rescan help text reflects whichever
  // one this device actually has.
  const isTouch = navigator.userAgent.includes("Android");

  const dismiss = backdropDismiss(() => onclose());

  let panel = $state<HTMLDivElement | null>(null);

  // One source of truth for the layout mode. The CSS below keys off the same
  // width, and the info affordance needs the answer in JS too (anchored popover
  // above it, bottom sheet below), so a narrow desktop window behaves like a
  // phone instead of the app sniffing the platform.
  const NARROW_QUERY = "(max-width: 600px)";
  let narrow = $state(false);
  $effect(() => {
    const mq = window.matchMedia(NARROW_QUERY);
    narrow = mq.matches;
    const onChange = () => (narrow = mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  });

  // Focus the panel so Escape lands here (and stops) instead of the global keymap.
  $effect(() => {
    panel?.focus();
  });

  // A sub-panel or an explanation left open would greet the next visit out of
  // context, so the dialog always opens at its top level.
  $effect(() => {
    view.resetSettingsNav();
    return () => view.resetSettingsNav();
  });

  let query = $state("");

  const openPanel = $derived(SETTINGS.find((s) => s.id === view.settingsPanel) ?? null);
  const infoSetting = $derived(SETTINGS.find((s) => s.id === view.settingsInfo) ?? null);

  /** Settings of a group that survive the current search. */
  function rows(group: GroupId): Setting[] {
    return SETTINGS.filter((s) => s.group === group && matches(s, query));
  }

  const visibleGroups = $derived(GROUPS.filter((g) => rows(g.id).length > 0));

  /** Split the groups into two desktop columns of roughly equal height, counting
   *  a header plus its rows. Balancing rather than hardcoding keeps the columns
   *  even as settings are added, and as the search empties groups out. */
  const columns = $derived.by<[SettingGroup[], SettingGroup[]]>(() => {
    const weight = (g: SettingGroup) => 1 + rows(g.id).length;
    const total = visibleGroups.reduce((n, g) => n + weight(g), 0);
    const left: SettingGroup[] = [];
    const right: SettingGroup[] = [];
    let filled = 0;
    for (const g of visibleGroups) {
      if (filled + weight(g) / 2 <= total / 2) {
        left.push(g);
        filled += weight(g);
      } else {
        right.push(g);
      }
    }
    return [left, right];
  });

  function onKeydown(e: KeyboardEvent) {
    // A key press ends any armed pointer release, so a control reached with Tab
    // and changed with the keyboard keeps its focus.
    pointerPress = false;
    e.stopPropagation();
    if (e.key !== "Escape") return;
    // Same order the app's back ladder uses: shed the innermost layer first.
    if (view.settingsInfo) view.settingsInfo = null;
    else if (view.settingsPanel) view.settingsPanel = null;
    else onclose();
  }

  // A pressed control must not keep focus: the ring lingers and the control
  // then swallows the next key press. Only a real pointer press arms the
  // release, so keyboard focus survives.
  let pointerPress = false;

  function releaseFocus(e: Event) {
    if (!pointerPress) return;
    pointerPress = false;
    (e.currentTarget as HTMLElement | null)?.blur();
    // Focus goes back to the panel so Escape keeps landing on this dialog
    // instead of reaching the global keymap.
    panel?.focus();
  }

  /** Wrap a control handler so a pointer press releases focus afterwards. */
  function releasing<E extends Event>(fn: (e: E) => void): (e: E) => void {
    return (e) => {
      fn(e);
      releaseFocus(e);
    };
  }

  // --- the info affordance ---

  let infoAnchor: HTMLElement | null = null;
  let infoEl = $state<HTMLDivElement | null>(null);

  function toggleInfo(id: string, e: MouseEvent) {
    infoAnchor = e.currentTarget as HTMLElement;
    view.settingsInfo = view.settingsInfo === id ? null : id;
    releaseFocus(e);
  }

  // Desktop only: park the popover under the icon it belongs to and let the
  // shared clamp pull it back inside the window. The bottom sheet needs none of
  // this, since it spans the full width by construction.
  $effect(() => {
    if (narrow || !view.settingsInfo || !infoEl || !infoAnchor) return;
    const r = infoAnchor.getBoundingClientRect();
    infoEl.style.left = `${r.left}px`;
    infoEl.style.top = `${r.bottom + 6}px`;
    return keepClamped(() => infoEl);
  });

  // --- preview quality ---

  // Preview quality invalidates every generated preview, so the select never
  // applies straight away: it parks the choice here and waits for a confirm.
  let pendingQuality = $state<number | null>(null);
  let previewBusy = $state(false);
  let previewMsg = $state("");

  function changePreviewQuality(e: Event) {
    const select = e.currentTarget as HTMLSelectElement;
    releaseFocus(e);
    const next = Number(select.value);
    if (next === settings.previewQuality) return;
    // Keep showing the value still in force until the change is confirmed.
    select.value = String(settings.previewQuality);
    pendingQuality = next;
  }

  async function applyPreviewQuality() {
    const next = pendingQuality;
    pendingQuality = null;
    if (next === null) return;
    previewBusy = true;
    previewMsg = "";
    try {
      settings.setPreviewQuality(next);
      await api.setPreviewQuality(next);
      if (!catalog.project) {
        previewMsg = "Saved. Previews are rebuilt the next time you open a project.";
        return;
      }
      // Best-effort: if this fails (a scan is running), the previews are merely
      // left on disk. The ingest regenerates them anyway, because a row whose
      // long_edge no longer matches the setting counts as missing.
      await api.discardPreviews();
      await api.rescanProject();
      previewMsg = "Rebuilding previews at the new size…";
    } catch (err) {
      previewMsg = String(err);
    } finally {
      previewBusy = false;
    }
  }

  // --- resets ---

  let confirmResetAll = $state(false);

  // --- settings that live in other surfaces ---

  /** Both destinations need a project, so the whole section waits for one. */
  const elsewhere = $derived(
    catalog.project
      ? [
          {
            what: "Thumbnail size, grouping, RAW+JPEG pairing, burst collapsing",
            where: "View panel",
            go: () => {
              onclose();
              view.mode = "grid";
              session.viewPanelOpen = true;
            },
          },
          {
            what: "Where deleted files go",
            where: "Commit dialog",
            go: () => {
              onclose();
              session.commitDialogOpen = true;
            },
          },
        ]
      : [],
  );
</script>

{#snippet infoButton(s: Setting)}
  <button
    class="info"
    class:on={view.settingsInfo === s.id}
    aria-label="What does {s.label} do?"
    onclick={(e) => toggleInfo(s.id, e)}
  >
    <Info size={13} />
  </button>
{/snippet}

{#snippet settingRow(s: Setting)}
  <div class="row" class:changed={s.modified()}>
    <span class="name">
      <span class="label">{s.label}</span>
      {@render infoButton(s)}
    </span>

    {#if s.kind === "toggle"}
      <button
        class="switch"
        class:on={s.get()}
        role="switch"
        aria-checked={s.get()}
        aria-label={s.label}
        onclick={releasing(() => s.set(!s.get()))}
      >
        <span class="knob"></span>
      </button>
    {:else if s.kind === "choice"}
      <select aria-label={s.label} value={s.get()} onchange={releasing((e) => s.set(e.currentTarget.value))}>
        {#each s.options as o (o.value)}
          <option value={o.value}>{o.label}</option>
        {/each}
      </select>
    {:else if s.kind === "panel"}
      <button class="drill" onclick={releasing(() => (view.settingsPanel = s.id))}>
        <span class="summary">{s.summary()}</span>
        <ChevronRight size={14} />
      </button>
    {:else if s.slot === "previewQuality"}
      <select
        aria-label={s.label}
        disabled={previewBusy}
        value={settings.previewQuality}
        onchange={changePreviewQuality}
      >
        {#each PREVIEW_QUALITY_CHOICES as choice (choice)}
          <option value={choice}>{choice} px · {PREVIEW_QUALITY_LABELS[choice]}</option>
        {/each}
      </select>
    {/if}
  </div>
{/snippet}

{#snippet groupBlock(g: SettingGroup)}
  <section class="group">
    <header>
      <g.icon size={14} />
      <span class="gname">{g.label}</span>
      {#if groupModified(g.id)}
        <button
          class="greset"
          title="Reset {g.label} to defaults"
          aria-label="Reset {g.label} to defaults"
          onclick={releasing(() => resetGroup(g.id))}
        >
          <RotateCcw size={12} />
        </button>
      {/if}
    </header>
    {#each rows(g.id) as s (s.id)}
      {@render settingRow(s)}
    {/each}
    {#if g.id === "quality" && previewMsg}
      <p class="hint">{previewMsg}</p>
    {/if}
  </section>
{/snippet}

<div class="backdrop" {...dismiss} role="presentation">
  <div
    class="dialog"
    bind:this={panel}
    onclick={(e) => e.stopPropagation()}
    onpointerdown={() => (pointerPress = true)}
    onkeydown={onKeydown}
    role="dialog"
    aria-label="Settings"
    tabindex="-1"
  >
    <header class="head">
      {#if openPanel}
        <button class="back" onclick={releasing(() => (view.settingsPanel = null))} aria-label="Back">
          <ChevronLeft size={18} />
        </button>
        <h2>{openPanel.label}</h2>
      {:else}
        <h2>Settings</h2>
        <label class="find">
          <Search size={13} />
          <input
            type="search"
            placeholder="Search settings"
            aria-label="Search settings"
            bind:value={query}
          />
        </label>
      {/if}
      <button class="close-x" onclick={onclose} aria-label="Close settings" title="Close">
        <X size={18} />
      </button>
    </header>

    {#if openPanel?.kind === "panel" && openPanel.panel === "filmstripBadges"}
      <div class="content sub">
        <p class="sub-intro">{openPanel.info}</p>
        {#each FILMSTRIP_BADGES as badge (badge.label)}
          <label class="check">
            <input
              type="checkbox"
              checked={badge.get()}
              onchange={releasing((e) => badge.set(e.currentTarget.checked))}
            />
            <span>{badge.label}</span>
          </label>
        {/each}
      </div>
    {:else if openPanel?.kind === "panel" && openPanel.panel === "touchBar"}
      <div class="content sub">
        <p class="sub-intro">{openPanel.info}</p>
        <DragList
          items={settings.bottomBarList}
          keyOf={(it) => it.id}
          onMove={(from, to) => settings.moveBottomBarItem(from, to)}
          ariaLabel="Bottom bar groups"
        >
          {#snippet row(it)}
            <label class="bar-row">
              <span class="bar-name" class:off={it.hidden}>{it.label}</span>
              <input
                type="checkbox"
                checked={!it.hidden}
                onchange={releasing(() => settings.toggleBottomBarHidden(it.id))}
              />
            </label>
          {/snippet}
        </DragList>
        <button class="wide" onclick={releasing(() => settings.resetBottomBar())}>
          <RotateCcw size={13} />
          <span>Reset to default</span>
        </button>
      </div>
    {:else}
      <div class="content">
        {#if visibleGroups.length === 0}
          <p class="empty">Nothing matches “{query}”.</p>
        {:else}
          <div class="cols">
            <div class="col">
              {#each columns[0] as g (g.id)}
                {@render groupBlock(g)}
              {/each}
            </div>
            <div class="col">
              {#each columns[1] as g (g.id)}
                {@render groupBlock(g)}
              {/each}
            </div>
          </div>
        {/if}

        {#if !query}
          <div class="jump">
            <button class="wide" onclick={releasing(onshowkeybindings)}>
              <Keyboard size={14} />
              <span>Keyboard shortcuts</span>
              <ChevronRight size={14} />
            </button>
            <button class="wide" onclick={releasing(onshowtags)}>
              <Tag size={14} />
              <span>Task tags</span>
              <ChevronRight size={14} />
            </button>
            <button class="wide support" onclick={releasing(onshowsupport)}>
              <Heart size={14} />
              <span>Support Cullant</span>
              <ChevronRight size={14} />
            </button>
          </div>

          {#if elsewhere.length > 0}
            <section class="group elsewhere">
              <header><span class="gname">Elsewhere</span></header>
              {#each elsewhere as item (item.where)}
                <button class="out" onclick={releasing(item.go)}>
                  <span class="what">{item.what}</span>
                  <span class="where">{item.where}<ArrowUpRight size={12} /></span>
                </button>
              {/each}
            </section>
          {/if}

          <p class="hint tail">
            {isTouch
              ? "Pull down on the grid to rescan the project now."
              : "Press ? anytime to see the shortcuts you have configured."}
          </p>

          {#if anyModified()}
            <button class="reset-all" onclick={releasing(() => (confirmResetAll = true))}>
              <RotateCcw size={13} />
              <span>Reset all settings</span>
            </button>
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</div>

{#if infoSetting}
  <!-- The explanation. Anchored under its icon on a wide screen, a full-width
       sheet on a narrow one, where a popover pinned near the right edge would
       leave the text a few characters wide. -->
  <div
    class="info-pop"
    class:sheet={narrow}
    bind:this={infoEl}
    role="tooltip"
    onpointerdown={(e) => e.stopPropagation()}
  >
    <span class="ip-title">{infoSetting.label}</span>
    <p>{infoSetting.info}</p>
  </div>
  <button
    class="info-scrim"
    aria-label="Close explanation"
    onclick={() => (view.settingsInfo = null)}
  ></button>
{/if}

{#if pendingQuality !== null}
  <ConfirmDialog
    title="Rebuild every preview?"
    message={catalog.project
      ? `Previews change to ${pendingQuality} px. The ones already generated are discarded and rebuilt in the background.`
      : `Previews change to ${pendingQuality} px, and are rebuilt the next time you open a project.`}
    confirmLabel="Change and rebuild"
    onconfirm={() => void applyPreviewQuality()}
    oncancel={() => (pendingQuality = null)}
  />
{/if}

{#if confirmResetAll}
  <ConfirmDialog
    title="Reset all settings?"
    message="Every preference goes back to the value Cullant ships with. Your photos, ratings and pending actions are untouched."
    confirmLabel="Reset all"
    onconfirm={() => {
      confirmResetAll = false;
      resetAll();
    }}
    oncancel={() => (confirmResetAll = false)}
  />
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
    padding: var(--dialog-edge-margin);
    padding-top: calc(var(--inset-top) + var(--dialog-edge-margin));
    padding-bottom: calc(var(--inset-bottom) + var(--dialog-edge-margin));
  }

  .dialog {
    outline: none;
    display: flex;
    flex-direction: column;
    /* Two columns of label-only rows fit every desktop screen without scrolling,
       which is the whole point of the layout: no navigation and no hidden state. */
    width: 720px;
    max-width: 100%;
    max-height: 100%;
    background: #232329;
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.5);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 8px 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .find {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface);
    opacity: 0.75;
  }

  .find:focus-within {
    opacity: 1;
    border-color: var(--accent);
  }

  .find input {
    width: 150px;
    padding: 5px 0;
    border: 0;
    background: none;
    color: inherit;
    font-size: 12px;
    outline: none;
  }

  /* The platform search affordance is a second, redundant clear button. */
  .find input::-webkit-search-cancel-button {
    display: none;
  }

  .back,
  .close-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 32px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    opacity: 0.6;
    cursor: pointer;
  }

  .close-x {
    margin-left: auto;
  }

  .back {
    margin-left: -6px;
  }

  .back:hover,
  .close-x:hover {
    opacity: 1;
    background: var(--surface);
  }

  .content {
    overflow-y: auto;
    padding: 14px 16px 16px;
  }

  .cols {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px 22px;
    align-items: start;
  }

  .col {
    min-width: 0;
  }

  .group {
    margin-bottom: 14px;
  }

  .group header {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 4px 2px;
    color: #cfcfd6;
  }

  .group header :global(svg) {
    opacity: 0.65;
  }

  .gname {
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    opacity: 0.75;
  }

  .greset {
    margin-left: auto;
    display: inline-flex;
    padding: 3px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    opacity: 0.5;
    cursor: pointer;
  }

  .greset:hover {
    opacity: 1;
    background: var(--surface);
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 2px 4px 2px 6px;
    border-radius: 6px;
  }

  .row:hover {
    background: var(--surface);
  }

  .name {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .label {
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* A dot on any preference that no longer matches what Cullant ships, so "what
     have I changed" is answerable at a glance when something behaves oddly. */
  .row.changed .label::after {
    content: "";
    display: inline-block;
    width: 5px;
    height: 5px;
    margin-left: 6px;
    vertical-align: middle;
    border-radius: 50%;
    background: var(--accent);
  }

  .info {
    display: inline-flex;
    flex: none;
    padding: 3px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    opacity: 0.32;
    cursor: pointer;
  }

  .row:hover .info,
  .info.on {
    opacity: 0.9;
  }

  .switch {
    flex: none;
    position: relative;
    width: 32px;
    height: 18px;
    padding: 0;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    background: var(--surface);
    cursor: pointer;
  }

  .switch.on {
    background: var(--accent);
    border-color: var(--accent);
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #d8d8de;
    transition: transform 0.14s ease;
  }

  .switch.on .knob {
    transform: translateX(14px);
    background: #fff;
  }

  select {
    flex: none;
    max-width: 46%;
    padding: 3px 6px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: inherit;
    font-size: 11.5px;
  }

  .drill {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 4px 3px 8px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    cursor: pointer;
  }

  .drill:hover {
    background: var(--surface-2);
  }

  .summary {
    font-size: 11.5px;
    opacity: 0.65;
  }

  .jump {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
    margin-top: 4px;
  }

  .wide {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 9px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    color: inherit;
    font-size: 12.5px;
    cursor: pointer;
  }

  .wide:hover {
    border-color: var(--border-strong);
    background: var(--surface-2);
  }

  .wide span {
    text-align: left;
  }

  .wide :global(svg:last-child) {
    margin-left: auto;
    opacity: 0.5;
  }

  .support :global(svg:first-child) {
    color: #ff7597;
  }

  .elsewhere {
    margin-top: 14px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
  }

  .out {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 6px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }

  .out:hover {
    background: var(--surface);
  }

  .what {
    flex: 1;
    opacity: 0.8;
  }

  .where {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    font-size: 11px;
    color: var(--accent);
  }

  .hint {
    margin: 6px 2px 0;
    font-size: 11px;
    opacity: 0.55;
  }

  .tail {
    margin-top: 12px;
  }

  .empty {
    margin: 24px 0;
    text-align: center;
    font-size: 12.5px;
    opacity: 0.6;
  }

  .reset-all {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 12px;
    padding: 6px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: none;
    color: inherit;
    font-size: 11.5px;
    opacity: 0.8;
    cursor: pointer;
  }

  .reset-all:hover {
    opacity: 1;
    border-color: #b4545c;
    color: #ff9ca3;
  }

  /* --- sub-panels --- */

  .sub {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .sub-intro {
    margin: 0 2px 8px;
    font-size: 12px;
    line-height: 1.45;
    opacity: 0.65;
  }

  .check,
  .bar-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 36px;
    padding: 0 6px;
    border-radius: 6px;
    font-size: 12.5px;
    cursor: pointer;
  }

  .check:hover,
  .bar-row:hover {
    background: var(--surface);
  }

  .bar-name {
    flex: 1;
  }

  .bar-name.off {
    opacity: 0.45;
    text-decoration: line-through;
  }

  .sub .wide {
    margin-top: 8px;
  }

  /* --- the explanation --- */

  .info-scrim {
    position: fixed;
    inset: 0;
    z-index: 61;
    border: 0;
    background: none;
    cursor: default;
  }

  .info-pop {
    position: fixed;
    z-index: 62;
    width: 300px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    overflow-y: auto;
    padding: 10px 12px;
    background: #2c2c33;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
  }

  .info-pop.sheet {
    left: 0;
    right: 0;
    bottom: 0;
    top: auto;
    width: auto;
    max-width: none;
    padding: 16px 18px calc(18px + var(--inset-bottom));
    border-width: 1px 0 0;
    border-radius: 14px 14px 0 0;
  }

  .ip-title {
    display: block;
    margin-bottom: 4px;
    font-size: 12px
;
    font-weight: 600;
  }

  .info-pop p {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    opacity: 0.8;
  }

  @media (max-width: 600px) {
    .backdrop {
      padding: 0;
    }

    .dialog {
      width: 100%;
      height: 100%;
      max-height: none;
      border: 0;
      border-radius: 0;
      padding-top: var(--inset-top);
    }

    /* One column, and the group header sticks so you always know which group the
       rows under your thumb belong to. */
    .cols {
      grid-template-columns: 1fr;
      gap: 0;
    }

    .group header {
      position: sticky;
      top: 0;
      z-index: 1;
      background: #232329;
    }

    .row {
      min-height: 44px;
    }

    .jump {
      grid-template-columns: 1fr;
    }

    .find input {
      width: 100%;
      min-width: 0;
    }

    .find {
      flex: 1;
      min-width: 0;
    }
  }
</style>
