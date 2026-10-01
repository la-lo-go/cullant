<script lang="ts">
  import { untrack } from "svelte";
  import { slide } from "svelte/transition";
  import {
    settings,
    COLOR_LABELS,
    type ColorLabel,
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
  import { LABEL_COLORS } from "../labels";
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";
  import { IS_TOUCH } from "../platform";
  import { getVersion } from "@tauri-apps/api/app";
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
    slotLabel,
    type RadialGroupId,
    type RadialMouse,
    type RadialSlot,
  } from "../radial";
  import type { CommandId } from "../keyboard/keymap";
  import { shortcutHint } from "../keyboard/hints";
  import { FREE_WAYS, MONEY_WAYS } from "../support";

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
  import DatabaseBackup from "@lucide/svelte/icons/database-backup";
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
    query = $bindable(""),
  }: {
    onclose: () => void;
    onshowkeybindings: () => void;
    onshowtags: () => void;
    onshowsupport: () => void;
    query?: string;
  } = $props();

  // A touch platform reaches the manual rescan via pull-to-refresh; desktop uses
  // the title-bar menu. The tail hint reflects whichever this has.
  const isTouch = IS_TOUCH;

  const dismiss = backdropDismiss(() => onclose());

  let panel = $state<HTMLDivElement | null>(null);

  // A sub-panel or an explanation left open would greet the next visit out of
  // context, so the dialog always opens at its top level.
  $effect(() => {
    view.resetSettingsNav();
    return () => view.resetSettingsNav();
  });

  let colorNameDrafts = $state<Partial<Record<ColorLabel, string>>>({ ...settings.colorLabelNames });

  const openPanel = $derived(SETTINGS.find((s) => s.id === view.settingsPanel) ?? null);
  let reorderHeld = $state(false);
  let heldResetVisible = $state(false);
  const showPanelReset = $derived(reorderHeld ? heldResetVisible : (openPanel?.modified() ?? false));

  function setReorderHeld(active: boolean) {
    if (active) heldResetVisible = openPanel?.modified() ?? false;
    reorderHeld = active;
  }

  function resetSlide(node: HTMLElement) {
    return slide(node, { duration: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? 0 : 200 });
  }

  function retireReset(e: Event) {
    const slot = e.currentTarget as HTMLElement;
    if (slot.contains(document.activeElement)) panel?.focus({ preventScroll: true });
    slot.inert = true;
  }

  $effect(() => {
    if (view.settingsPanel === "colorLabelNames") {
      colorNameDrafts = untrack(() => ({ ...settings.colorLabelNames }));
    }
  });

  function rows(group: GroupId): Setting[] {
    return SETTINGS.filter((s) => s.kind !== "panel" && s.group === group && matches(s, query));
  }

  const visibleGroups = $derived(GROUPS.filter((g) => rows(g.id).length > 0));

  /** The panels, gathered out of their groups. Each is a door onto a screenful
   *  of its own, so they belong with the other doors at the foot of the dialog
   *  rather than sitting as a card in the middle of a column of preferences. */
  const panelRows = $derived(SETTINGS.filter((s) => s.kind === "panel" && matches(s, query)));

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
    clearPointerMode();
    e.stopPropagation();
    if (e.key !== "Escape") return;
    // Same order the app's back ladder uses: shed the innermost layer first.
    if (view.infoTip) view.infoTip = null;
    else if (view.settingsPanel) view.settingsPanel = null;
    else onclose();
  }

  let pointerPress = false;
  let pointerMode = $state(false);
  let touchPointer = $state(false);

  function clearPointerMode() {
    pointerPress = false;
    pointerMode = false;
    touchPointer = false;
  }

  function onPointerPress(e: PointerEvent) {
    pointerPress = true;
    pointerMode = true;
    touchPointer = e.pointerType === "touch";
  }

  function releaseFocus(e?: Event) {
    if (!pointerPress) return;
    pointerPress = false;
    (e?.currentTarget as HTMLElement | null)?.blur();
    panel?.focus({ preventScroll: true });
  }

  function releaseClickedControl(e: MouseEvent) {
    e.stopPropagation();
    if (!pointerPress || e.detail === 0 || !(e.target instanceof Element)) return;
    const control = e.target.closest<HTMLElement>('button, input[type="checkbox"], input[type="radio"]');
    if (!control || control.matches(":disabled")) return;
    queueMicrotask(() => {
      if (!pointerPress || !panel?.isConnected) return;
      pointerPress = false;
      const focused = document.activeElement;
      control.blur();
      if (focused === control || focused === document.body) panel.focus({ preventScroll: true });
    });
  }

  function releasing<E extends Event>(fn: (e: E) => void): (e: E) => void {
    return (e) => {
      fn(e);
      releaseFocus(e);
    };
  }

  // Preview quality invalidates every generated preview, so the select never
  // applies straight away: it parks the choice here and waits for a confirm.
  let pendingQuality = $state<number | null>(null);
  let previewBusy = $state(false);
  let previewMsg = $state("");
  let appVersion = $state("");

  $effect(() => {
    void getVersion()
      .then((version) => (appVersion = version))
      .catch(() => (appVersion = "unknown"));
  });

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
    if (catalog.ingesting) {
      previewMsg = "Wait for photo preparation to finish, then change the preview size.";
      return;
    }
    previewBusy = true;
    previewMsg = "";
    try {
      settings.setPreviewQuality(next);
      catalog.invalidatePreviewReady();
      await api.setPreviewQuality(next);
      if (!catalog.project) {
        previewMsg = "Saved. Previews are rebuilt the next time you open a project.";
        return;
      }
      // Discard cached previews only after the active import has finished.
      await api.discardPreviews();
      await api.rescanProject();
      previewMsg = "Rebuilding previews at the new size…";
    } catch (err) {
      previewMsg = String(err);
    } finally {
      if (catalog.project) await catalog.refreshPreviewReady();
      previewBusy = false;
    }
  }

  let confirmResetAll = $state(false);
  let confirmReimport = $state(false);

  const supportDestination = {
    label: "Support Cullant",
    info: [...FREE_WAYS, ...MONEY_WAYS].map(({ label, note }) => `${label} ${note}`).join(" "),
    keywords: "support help contribution donate donation free",
  };
  const resetDestination = {
    label: "Reset all settings",
    info: "Restore the settings in this dialog to their defaults.",
    keywords: "reset defaults preferences restore",
  };
  const reimportDestination = {
    label: "Reimport this project",
    info: "Clear the project catalog and read its folder again.",
    keywords: "reimport rebuild reset project database repair recover",
  };
  const showSupport = $derived(matches(supportDestination, query));
  const showReset = $derived(matches(resetDestination, query) && anyModified());
  const showReimport = $derived(catalog.project !== null && matches(reimportDestination, query));

  const destinations = $derived([
    {
      label: "Keyboard shortcuts",
      info: "Search and change command keys. Reset one shortcut or all shortcuts.",
      keywords: "keyboard keymap bindings remap hotkeys",
      icon: Keyboard,
      go: onshowkeybindings,
    },
    {
      label: "Task tags",
      info: "Create or edit task tag names, colors, scope and shortcuts.",
      keywords: "tags labels keywords",
      icon: Tag,
      go: onshowtags,
    },
  ].filter((item) => matches(item, query)));

  const elsewhere = $derived(
    (catalog.project
      ? [
          {
            label: "View panel",
            info: "Thumbnail size, framing, file names, grouping, RAW+JPEG pairing, burst collapsing",
            keywords: "filename filenames photos grid fit fill",
            go: () => {
              onclose();
              view.mode = "grid";
              session.viewPanelOpen = true;
            },
          },
          {
            label: "Review changes",
            info: "Pending actions, commit history, and where deleted files go",
            keywords: "delete trash queue move copy undo destination",
            go: () => {
              onclose();
              session.commitDialogOpen = true;
            },
          },
        ]
      : []).filter((item) => matches(item, query)),
  );
</script>

{#snippet panelReset(label: string, reset: () => void)}
  {#if showPanelReset}
    <div
      class="reset-slot"
      transition:resetSlide
      onintrostart={(e) => (e.currentTarget.inert = false)}
      onoutrostart={retireReset}
    >
      <button class="wide" onclick={releasing(reset)}>
        <RotateCcw size={13} />
        <span>{label}</span>
      </button>
    </div>
  {/if}
{/snippet}

{#snippet settingRow(s: Setting)}
  {#if s.kind === "panel"}
    <!-- A door, drawn like the doors at the foot of the dialog rather than like
         a preference sitting next to its control. It leads somewhere with its
         own screenful of settings, and that was not visible when it was a row
         with a small chevron on the end of it. No info affordance: the panel
         opens with the same explanation as its first line. -->
    <button
      class="wide door"
      class:changed={s.modified()}
      onclick={releasing(() => (view.settingsPanel = s.id))}
    >
      <s.icon size={14} />
      <span>{s.label}</span>
      <ChevronRight size={14} />
    </button>
  {:else}
    <!-- A toggle row IS the label of its checkbox, so the whole row flips it and
         the switch stays one real control with one tab stop. The info button
         inside is interactive content, which a label does not forward to. -->
    <svelte:element
      this={s.kind === "toggle" ? "label" : "div"}
      class="row"
      class:changed={s.modified()}
      class:whole-row={s.kind === "toggle"}
    >
      {#if s.kind === "toggle"}
        <input
          class="sw-input"
          type="checkbox"
          aria-label={s.label}
          checked={s.get()}
          onchange={releasing((e) => s.set(e.currentTarget.checked))}
        />
      {/if}
      <span class="name">
        <span class="label">{s.label}</span>
        <InfoTip title={s.label} text={s.info} />
      </span>

      {#if s.kind === "toggle"}
        <span class="switch" class:on={s.get()}>
          <span class="knob"></span>
        </span>
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
    </svelte:element>
  {/if}
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
    class:pointer-mode={pointerMode}
    class:touch-pointer={touchPointer}
    bind:this={panel}
    use:modalFocus={{ onKeyboardInteraction: clearPointerMode }}
    onclick={releaseClickedControl}
    onpointerdowncapture={onPointerPress}
    onpointermovecapture={(e) => { if (e.pointerType === "mouse") touchPointer = false; }}
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
        {@render panelReset("Reset filmstrip badges", () => openPanel?.reset())}
      </div>
    {:else if openPanel?.kind === "panel" && openPanel.panel === "colorLabelNames"}
      <div class="content sub">
        <p class="sub-intro">{openPanel.info}</p>
        {#each COLOR_LABELS as label (label)}
          <label class="label-name">
            <span class="label-swatch" style:background={LABEL_COLORS[label]} title={label} aria-hidden="true"></span>
            <input
              type="text"
              aria-label={`${label} label name`}
              placeholder={label}
              maxlength="40"
              bind:value={colorNameDrafts[label]}
              oninput={(e) => settings.setColorLabelName(label, e.currentTarget.value)}
            />
          </label>
        {/each}
        {@render panelReset("Reset color label names", () => {
          colorNameDrafts = {};
          settings.resetColorLabelNames();
        })}
      </div>
    {:else if openPanel?.kind === "panel" && openPanel.panel === "touchBar"}
      <div class="content sub">
        <p class="sub-intro">{openPanel.info}</p>
        <DragList
          items={settings.bottomBarList}
          keyOf={(it) => it.id}
          itemLabel={(it) => it.label}
          onMove={(from, to) => settings.moveBottomBarItem(from, to)}
          onInteractionChange={setReorderHeld}
          onPointerRelease={() => releaseFocus()}
          ariaLabel="Bottom bar groups"
        >
          {#snippet row(it)}
            <label class="check">
              <span class="bar-label" class:off={it.hidden}>
                {#if it.icon}<it.icon size={16} />{/if}
                <span>{it.label}</span>
              </span>
              <input
                type="checkbox"
                checked={!it.hidden}
                onchange={releasing(() => settings.toggleBottomBarHidden(it.id))}
              />
            </label>
          {/snippet}
        </DragList>
        {@render panelReset("Reset to default", () => settings.resetBottomBar())}
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
              itemLabel={(it, i) => `sector ${i + 1}: ${slotLabel(it.slot)}`}
              onMove={(from, to) => settings.moveRadialSlot(from, to)}
              onInteractionChange={setReorderHeld}
              onPointerRelease={() => releaseFocus()}
              ariaLabel="Radial menu sectors"
            >
              {#snippet row(it)}
                <div
                  class="slot"
                  class:on={radialHighlight === it.i}
                  role="presentation"
                  onpointerenter={(e) => { if (e.pointerType === "mouse") radialHighlight = it.i; }}
                  onpointerleave={() => (radialHighlight = -1)}
                >
                  <!-- Every sector is the same control, `more` included: it is
                       one of the things a sector can be, so making it a line of
                       grey text was a special case with nothing behind it. -->
                  <select
                    aria-label={`Sector ${it.i + 1} action`}
                    value={slotKey(it.slot)}
                    onchange={releasing((e) => {
                      const next = e.currentTarget.value;
                      // A keyed row can move without Svelte updating its select value.
                      e.currentTarget.value = slotKey(it.slot);
                      assignSlot(it.i, next);
                    })}
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
                    title={settings.radialSlots.length <= RADIAL_MIN_SECTORS ? "Keep at least two sectors." : `Remove sector ${it.i + 1}: ${slotLabel(it.slot)}`}
                    aria-label={`Remove sector ${it.i + 1}: ${slotLabel(it.slot)}`}
                    disabled={settings.radialSlots.length <= RADIAL_MIN_SECTORS}
                    onclick={releasing(() => settings.removeRadialSlot(it.i))}
                  >
                    <X size={14} />
                  </button>
                </div>
              {/snippet}
            </DragList>

            <p class="hint">Keep at least two sectors. More opens the full command list and can be removed.</p>

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
                aria-label="Radial menu opens with"
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
                aria-label="Radial menu rotation"
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

        {@render panelReset("Reset to default", () => settings.resetRadial())}
      </div>
    {:else}
      <div class="content">
        {#if visibleGroups.length === 0 && panelRows.length === 0 && destinations.length === 0 && elsewhere.length === 0 && !showSupport && !showReset && !showReimport}
          <p class="empty">Nothing matches “{query}”.</p>
        {:else if visibleGroups.length > 0}
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

        {#if panelRows.length > 0 || destinations.length > 0}
          <div class="jump">
            {#each panelRows as s (s.id)}
              {@render settingRow(s)}
            {/each}
            {#each destinations as item (item.label)}
              <button class="wide" onclick={releasing(item.go)}>
                <item.icon size={14} />
                <span>{item.label}</span>
                <ChevronRight size={14} />
              </button>
            {/each}
          </div>
        {/if}

        {#if showSupport}
          <!-- Deliberately not a third grey row like the two above. Nothing funds
               this app, so the one ask it makes gets to be seen. -->
          <button class="support" onclick={releasing(onshowsupport)}>
            <Heart size={20} />
            <span class="s-text">
              <span class="s-title">Support Cullant</span>
              <span class="s-note">
                Support development with a donation or help with feedback.
              </span>
            </span>
            <ChevronRight size={16} />
          </button>
        {/if}

        {#if elsewhere.length > 0}
            <section class="group elsewhere">
              <header><span class="gname">Elsewhere</span></header>
              {#each elsewhere as item (item.label)}
                <button class="out" onclick={releasing(item.go)}>
                  <span class="what">{item.info}</span>
                  <span class="where">{item.label}<ArrowUpRight size={12} /></span>
                </button>
              {/each}
            </section>
        {/if}

        {#if !query}
          <p class="hint tail">
            {isTouch
              ? "Pull down on the grid to rescan the project now."
              : shortcutHint("Show configured shortcuts", "ui.toggleShortcuts")}
          </p>
        {/if}

          {#if showReset}
            <button
              class="reset-all"
              onclick={releasing(() => (confirmResetAll = true))}
            >
              <RotateCcw size={13} />
              <span>Reset all settings</span>
            </button>
          {/if}
          {#if showReimport}
            <!-- The escape hatch for a project that has gone strange. Down here
                 with the other reset, because it is one: it throws away what
                 Cullant stored, not a preference. -->
            <button class="reset-all" onclick={releasing(() => (confirmReimport = true))}>
              <DatabaseBackup size={13} />
              <span>Reimport this project…</span>
            </button>
          {/if}
        {#if !query}
          <p class="version">Cullant {appVersion || "…"}</p>
        {/if}
      </div>
    {/if}
    {#if addMenu}
      <ContextMenu
        x={addMenu.x}
        y={addMenu.y}
        items={addMenu.items}
        onclose={() => (addMenu = null)}
      />
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

{#if confirmReimport}
  <ConfirmDialog
    title="Reimport this project?"
    message="Cullant forgets everything it has stored about this project and reads the folder again from nothing: every rating, flag, colour label and tag, the queue of pending actions, and the commit history. Your photos and videos are not touched, and anything already exported to XMP sidecars comes back on the rescan."
    confirmLabel="Reimport"
    onconfirm={() => {
      confirmReimport = false;
      onclose();
      void catalog.reimport();
    }}
    oncancel={() => (confirmReimport = false)}
  />
{/if}

{#if confirmResetAll}
  <ConfirmDialog
    title="Reset all settings?"
    message="Reset the settings and submenu settings in this dialog to their defaults. View settings, keyboard shortcuts, task tags, and project file actions stay as they are."
    confirmLabel="Reset all"
    onconfirm={() => {
      confirmResetAll = false;
      resetAll();
    }}
    oncancel={() => (confirmResetAll = false)}
  />
{/if}

<style>
  .label-name {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 8px 0;
  }

  .label-swatch {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    flex: none;
  }

  .label-name input {
    min-width: 0;
    flex: 1;
    padding: 7px 9px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--control);
    color: inherit;
    font: inherit;
  }

  .label-name input:focus {
    outline: none;
    border-color: var(--accent);
  }

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

  .dialog.pointer-mode :global(select:focus) {
    outline: none;
    border-color: var(--border-strong);
  }

  .dialog.pointer-mode .sw-input:focus-visible ~ .switch {
    outline: none;
  }

  .dialog.touch-pointer :global(select:hover) {
    background-color: var(--control);
  }

  .dialog.touch-pointer :global(.draglist:not(.reordering) .handle:hover) {
    color: #6a6a72;
  }

  .dialog.touch-pointer :global(.tip:not(.open):hover) {
    opacity: 0.35;
    color: inherit;
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
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    padding: 0;
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
    box-sizing: border-box;
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
    top: 50%;
    left: 2px;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #d8d8de;
    transform: translateY(-50%);
    transition: transform 0.14s ease;
  }

  .switch.on .knob {
    transform: translate(14px, -50%);
    background: #fff;
  }

  select {
    flex: none;
    max-width: 46%;
    padding: 3px 28px 3px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background-color: var(--surface);
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

  .version {
    margin: 12px 0 0;
    color: #74747d;
    font-size: 10.5px;
    text-align: center;
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

  .bar-label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  .reset-slot {
    flex-shrink: 0;
    padding-top: 8px;
  }

  /* Hybrid devices can retain hover after a touch. */
  @media (hover: hover) {
    .dialog:not(.touch-pointer) .back:hover,
    .dialog:not(.touch-pointer) .close-x:hover {
      opacity: 1;
      background: var(--hover);
    }

    .dialog:not(.touch-pointer) .row:hover,
    .dialog:not(.touch-pointer) .out:hover,
    .dialog:not(.touch-pointer) .check:hover {
      background: var(--hover);
    }

    .dialog:not(.touch-pointer) .greset:hover {
      opacity: 1;
      background: var(--hover);
    }

    .dialog:not(.touch-pointer) .wide:hover {
      background: var(--hover);
    }

    .dialog:not(.touch-pointer) .support:hover {
      background: var(--hover);
    }

    .dialog:not(.touch-pointer) .reset-all:hover {
      opacity: 1;
      background: var(--hover);
      color: #ff9ca3;
    }

    .dialog:not(.touch-pointer) .slot .drop:hover:not(:disabled),
    .dialog:not(.touch-pointer) .radial-rows .add:hover:not(:disabled) {
      color: #fff;
      background: var(--hover);
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
    box-sizing: border-box;
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

  /* Whole-row toggling: the row is the checkbox's label. */
  .row.whole-row {
    cursor: pointer;
  }

  /* Off screen but still focusable, so the switch keeps its place in the tab
     order and its focus ring — which the visual switch borrows below. */
  .sw-input {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
    pointer-events: none;
  }

  .sw-input:focus-visible ~ .switch {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .wide.door span:first-of-type {
    flex: 1;
    text-align: left;
  }

  .wide.door.changed span:first-of-type::after {
    content: "";
    display: inline-block;
    width: 5px;
    height: 5px;
    margin-left: 6px;
    border-radius: 50%;
    background: var(--accent);
    vertical-align: middle;
  }

  /* Values end at the edge their controls hug. Left-aligned inside boxes of
     four different widths, they started in four different places. */
  .row select {
    text-align: right;
    text-align-last: right;
  }
</style>
