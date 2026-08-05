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
  import InfoTip from "./InfoTip.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import { api } from "../api";
  import { backdropDismiss } from "../backdrop";
  import DragList from "./DragList.svelte";
  import RadialPreview from "./RadialPreview.svelte";
  import ContextMenu from "./ContextMenu.svelte";
  import { pruneMenu, type MenuNode } from "../menu";
  import {
    ASSIGNABLE_COMMANDS,
    RADIAL_GROUPS,
    RADIAL_MAX_SECTORS,
    RADIAL_MIN_SECTORS,
    RADIAL_ROTATION_CHOICES,
    slotKey,
    type RadialGroupId,
    type RadialMouse,
    type RadialSlot,
  } from "../radial";
  import type { CommandId } from "../keyboard/keymap";

  /** Which sector the panel is pointing at, so hovering a row lights up the
   *  matching wedge in the preview and the other way round. */
  let radialHighlight = $state(-1);

  /** The "what should this sector do?" menu, for adding one. */
  let addMenu = $state<{ x: number; y: number; items: MenuNode[] } | null>(null);

  /** Every action not already on the ring, as a menu. Asking first means a new
   *  sector is what the user wanted rather than a guess they have to correct. */
  function slotChoices(add: (slot: RadialSlot) => void): MenuNode[] {
    const free = (slot: RadialSlot) => !settings.radialHas(slot);
    return pruneMenu([
      ...(free({ kind: "more" })
        ? [
            {
              kind: "item" as const,
              label: "More… (the full command list)",
              run: () => add({ kind: "more" }),
            },
            { kind: "sep" as const },
          ]
        : []),
      {
        kind: "submenu",
        label: "Groups",
        children: RADIAL_GROUPS.filter((g) => free({ kind: "group", id: g.id })).map((g) => ({
          kind: "item" as const,
          label: g.label,
          run: () => add({ kind: "group", id: g.id }),
        })),
      },
      {
        kind: "submenu",
        label: "Commands",
        children: ASSIGNABLE_COMMANDS.filter((c) => free({ kind: "command", id: c.id })).map(
          (c) => ({
            kind: "item" as const,
            label: c.title,
            run: () => add({ kind: "command", id: c.id }),
          }),
        ),
      },
    ]);
  }

  function openAddMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    addMenu = {
      x: Math.round(r.left),
      y: Math.round(r.bottom + 4),
      items: slotChoices((slot) => settings.addRadialSlot(slot)),
    };
  }

  /** Turn a `<select>` value back into the slot it names. Assigning a slot that
   *  is already elsewhere swaps the two, which the store handles. */
  function assignSlot(index: number, key: string) {
    const [kind, id] = key.split(":", 2);
    const slot: RadialSlot =
      key === "more"
        ? { kind: "more" }
        : kind === "group"
          ? { kind: "group", id: id as RadialGroupId }
          : { kind: "command", id: id as CommandId };
    settings.setRadialSlot(index, slot);
  }
  import Search from "@lucide/svelte/icons/search";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Tag from "@lucide/svelte/icons/tag";
  import Heart from "@lucide/svelte/icons/heart";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import X from "@lucide/svelte/icons/x";
  import Plus from "@lucide/svelte/icons/plus";

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
  // desktop uses the title-bar menu. The tail hint reflects whichever this has.
  const isTouch = navigator.userAgent.includes("Android");

  const dismiss = backdropDismiss(() => onclose());

  let panel = $state<HTMLDivElement | null>(null);

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
    if (view.infoTip) view.infoTip = null;
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

  let confirmResetAll = $state(false);

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

{#snippet settingRow(s: Setting)}
  <div class="row" class:changed={s.modified()}>
    <span class="name">
      <span class="label">{s.label}</span>
      <InfoTip title={s.label} text={s.info} />
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
      <select
        aria-label={s.label}
        value={s.get()}
        onchange={releasing((e) => s.set(e.currentTarget.value))}
      >
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
        <button
          class="back"
          onclick={releasing(() => (view.settingsPanel = null))}
          aria-label="Back"
        >
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
            <span>{badge.label}</span>
            <input
              type="checkbox"
              checked={badge.get()}
              onchange={releasing((e) => badge.set(e.currentTarget.checked))}
            />
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
            <label class="check">
              <span class:off={it.hidden}>{it.label}</span>
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
    {:else if openPanel?.kind === "panel" && openPanel.panel === "radial"}
      <div class="content sub">
        <p class="sub-intro">{openPanel.info}</p>

        <div class="radial-editor">
          <!-- Beside the list on a wide dialog, above it when there is no room:
               the ring is what is being edited, so it should be in view while
               the rows are changed. -->
          <RadialPreview
            slots={settings.radialSlots}
            highlight={radialHighlight}
            onhighlight={(i) => (radialHighlight = i)}
          />

          <div class="radial-rows">
            <DragList
              items={settings.radialSlots.map((slot, i) => ({ slot, i }))}
              keyOf={(it) => slotKey(it.slot)}
              onMove={(from, to) => settings.moveRadialSlot(from, to)}
              ariaLabel="Radial menu sectors"
            >
              {#snippet row(it)}
                <div
                  class="slot"
                  class:on={radialHighlight === it.i}
                  role="presentation"
                  onpointerenter={() => (radialHighlight = it.i)}
                  onpointerleave={() => (radialHighlight = -1)}
                >
                  <!-- Every sector is the same control, `more` included: it is
                       one of the things a sector can be, so making it a line of
                       grey text was a special case with nothing behind it. -->
                  <select
                    value={slotKey(it.slot)}
                    onchange={releasing((e) => assignSlot(it.i, e.currentTarget.value))}
                  >
                    <option value="more">More… (the full command list)</option>
                    <optgroup label="Groups">
                      {#each RADIAL_GROUPS as g (g.id)}
                        <option value={`group:${g.id}`}>{g.label}</option>
                      {/each}
                    </optgroup>
                    <optgroup label="Commands">
                      {#each ASSIGNABLE_COMMANDS as c (c.id)}
                        <option value={`cmd:${c.id}`}>{c.title}</option>
                      {/each}
                    </optgroup>
                  </select>
                  <button
                    class="drop"
                    title="Remove this sector"
                    aria-label="Remove this sector"
                    disabled={settings.radialSlots.length <= RADIAL_MIN_SECTORS}
                    onclick={releasing(() => settings.removeRadialSlot(it.i))}
                  >
                    <X size={14} />
                  </button>
                </div>
              {/snippet}
            </DragList>

            <!-- Quiet, and left where the list ends: adding a sector is a step
                 you take now and then, not the thing this panel is for. As a
                 full-width bar it outweighed the sectors above it and split the
                 options below off from them. -->
            <button
              class="add"
              disabled={settings.radialSlots.length >= RADIAL_MAX_SECTORS}
              onclick={openAddMenu}
            >
              <Plus size={12} />
              <span>Add sector…</span>
            </button>

            <!-- Below the ring's own contents, and marked off from them: these
                 are about the ring rather than in it. -->
            <div class="slot ring-opt">
              <span class="slot-label">Opens with</span>
              <select
                value={settings.radialMouse}
                onchange={releasing((e) =>
                  settings.setRadialMouse(e.currentTarget.value as RadialMouse))}
              >
                <option value="left">Hold left button</option>
                <option value="right">Right button</option>
                <option value="both">Either button</option>
              </select>
            </div>

            <div class="slot">
              <span class="slot-label">Rotation</span>
              <select
                value={settings.radialRotation}
                onchange={releasing((e) =>
                  settings.setRadialRotation(Number(e.currentTarget.value)))}
              >
                {#each RADIAL_ROTATION_CHOICES as deg (deg)}
                  <option value={deg}>{deg}°</option>
                {/each}
              </select>
            </div>
          </div>
        </div>

        <button class="wide" onclick={releasing(() => settings.resetRadial())}>
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
          </div>

          <!-- Deliberately not a third grey row like the two above. Nothing funds
               this app, so the one ask it makes gets to be seen. -->
          <button class="support" onclick={releasing(onshowsupport)}>
            <Heart size={20} />
            <span class="s-text">
              <span class="s-title">Support Cullant</span>
              <span class="s-note">
                Free, no ads, no account. Help with your time or with money.
              </span>
            </span>
            <ChevronRight size={16} />
          </button>

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

{#if addMenu}
  <ContextMenu
    x={addMenu.x}
    y={addMenu.y}
    items={addMenu.items}
    onclose={() => (addMenu = null)}
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

  /* A pressed control keeps focus for the keyboard, never a visible ring. */
  .dialog :global(*:focus:not(:focus-visible)) {
    outline: none;
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

  /* The scroll container carries no top padding: a sticky group header offsets
     from the scrollport, so padding there becomes a transparent band with the
     previous group's rows sliding through it. The breathing room goes on the
     first group instead, which scrolls away like content should. */
  .content {
    overflow-y: auto;
    padding: 0 16px 22px;
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

  .col > .group:first-child {
    margin-top: 12px;
  }

  .group header {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 5px 2px;
    color: var(--accent);
  }

  .gname {
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
  }

  .greset {
    margin-left: auto;
    display: inline-flex;
    padding: 3px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    opacity: 0.6;
    cursor: pointer;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 2px 4px 2px 6px;
    border-radius: 6px;
  }

  .name {
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .label {
    position: relative;
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* A dot on any preference that no longer matches what Cullant ships, so "what
     have I changed" is answerable at a glance. It rides the top-right corner of
     the label like a superscript, not the middle of the line. */
  .row.changed .label {
    padding-right: 9px;
  }

  .row.changed .label::after {
    content: "";
    position: absolute;
    top: 0;
    right: 1px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--accent);
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

  .wide span {
    text-align: left;
  }

  .wide :global(svg:last-child) {
    margin-left: auto;
    opacity: 0.5;
  }

  .support {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    margin-top: 10px;
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
    border-radius: 10px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .support :global(svg:first-child) {
    flex: none;
    color: #ff7597;
  }

  .support :global(svg:last-child) {
    flex: none;
    margin-left: auto;
    opacity: 0.6;
  }

  .s-text {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }

  .s-title {
    font-size: 13px;
    font-weight: 600;
  }

  .s-note {
    font-size: 11.5px;
    line-height: 1.4;
    opacity: 0.7;
  }

  .elsewhere {
    margin-top: 16px;
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

  /* --- sub-panels --- */

  .sub {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding-top: 12px;
  }

  .sub-intro {
    margin: 0 2px 8px;
    font-size: 12px;
    line-height: 1.45;
    opacity: 0.65;
  }

  /* Control on the right, matching every row in the main list, so the eye runs
     down one column of controls instead of two. */
  .check {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 38px;
    padding: 0 6px;
    border-radius: 6px;
    font-size: 12.5px;
    cursor: pointer;
  }

  .check input {
    flex: none;
    width: 16px;
    height: 16px;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .check .off {
    opacity: 0.45;
    text-decoration: line-through;
  }

  .sub .wide {
    margin-top: 8px;
  }

  /* Hover belongs to pointers that can hover. On touch it sticks after a tap and
     reads as "this row is still selected". */
  @media (hover: hover) {
    .back:hover,
    .close-x:hover {
      opacity: 1;
      background: var(--surface);
    }

    .row:hover,
    .out:hover,
    .check:hover {
      background: var(--surface);
    }

    .greset:hover {
      opacity: 1;
      background: var(--surface);
    }

    .drill:hover {
      background: var(--surface-2);
    }

    .wide:hover {
      border-color: var(--border-strong);
      background: var(--surface-2);
    }

    .support:hover {
      border-color: var(--accent);
      background: color-mix(in srgb, var(--accent) 20%, transparent);
    }

    .reset-all:hover {
      opacity: 1;
      border-color: #b4545c;
      color: #ff9ca3;
    }
  }

  @media (max-width: 600px) {
    /* Still a card, not a full-screen takeover: the edge margin and the safe-area
       insets on the backdrop keep it clear of the screen edges, and the dialog
       just takes the width available. */
    .dialog {
      width: 100%;
    }

    /* One column, and the group header sticks so you always know which group the
       rows under your thumb belong to. Full-bleed, so nothing slides past it
       through a gap at the sides. */
    .cols {
      grid-template-columns: 1fr;
      gap: 0;
    }

    .group header {
      position: sticky;
      top: 0;
      z-index: 1;
      margin: 0 -16px;
      padding: 7px 18px;
      background: #232329;
    }

    .col > .group:first-child {
      margin-top: 0;
    }

    .row {
      min-height: 44px;
    }

    .check {
      min-height: 46px;
    }

    .jump {
      grid-template-columns: 1fr;
    }

    .find {
      flex: 1;
      min-width: 0;
    }

    .find input {
      width: 100%;
      min-width: 0;
    }
  }

  /* Radial editor: the ring beside the rows, stacking above them when the
     dialog is too narrow to hold both. */
  .radial-editor {
    display: flex;
    gap: 14px;
    align-items: flex-start;
  }

  .radial-rows {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .slot {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 2px 4px;
    border-radius: 6px;
  }

  .slot.on {
    background: var(--hover);
  }

  /* The select takes the row, so the highlight ends at the row's own controls
     instead of running on to the panel edge past everything in it. */
  .slot select {
    flex: 1;
    min-width: 0;
    width: 100%;
    max-width: none;
  }

  /* Same width as a sector row's drag handle plus its gap, so these labels line
     the selects up with the ones above rather than beside them. */
  .slot-label {
    flex: none;
    width: 74px;
    color: #8a8a93;
    font-size: 12px;
  }

  .slot .drop {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    flex: none;
    padding: 0;
    background: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: #9aa0a0;
    cursor: pointer;
  }

  .slot .drop:hover:not(:disabled) {
    color: #fff;
    border-color: var(--border-strong);
  }

  .slot .drop:disabled {
    opacity: 0.35;
    cursor: default;
  }

  @media (max-width: 620px) {
    .radial-editor {
      flex-direction: column;
      align-items: center;
    }

    .radial-rows {
      width: 100%;
    }
  }

  .radial-rows .add {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 2px;
    padding: 3px 8px;
    background: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: #9aa0a0;
    font-size: 11px;
    cursor: pointer;
  }

  .radial-rows .add:hover:not(:disabled) {
    color: #fff;
    border-color: var(--border-strong);
  }

  .radial-rows .add:disabled {
    opacity: 0.4;
    cursor: default;
  }

  /* A hairline, not a heading: the two rows under it belong to the same block,
     they are just about the ring rather than in it. */
  .slot.ring-opt {
    margin-top: 8px;
    padding-top: 10px;
    border-top: 1px solid var(--border);
    border-radius: 0;
  }
</style>
