<script lang="ts">
  import { LABEL_COLORS } from "../labels";
  import { session } from "$lib/stores/session.svelte";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { tags } from "$lib/stores/tags.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { markPressed, pressed, runCommand } from "$lib/keyboard/dispatcher.svelte";
  import { shortcutHint } from "$lib/keyboard/hints";
  import { formatColorLabel } from "$lib/colorLabels";
  import { tooltips } from "$lib/tooltips";
  import { DOCK_IDLE, dock, isVertical, type Side } from "$lib/stores/dock.svelte";
  import DockGrip from "./DockGrip.svelte";
  import DockGhost from "./DockGhost.svelte";
  import TouchActionBar from "./TouchActionBar.svelte";
  import type { CommandId } from "$lib/keyboard/keymap";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Star from "@lucide/svelte/icons/star";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Eraser from "@lucide/svelte/icons/eraser";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import Undo2 from "@lucide/svelte/icons/undo-2";

  // On touch devices the bar shows itself (coarse pointer). On desktop it is
  // opt-in: the parent flips `forceShow` from a toolbar toggle. `hidden` lets
  // the parent suppress the bar in the grid until there's a selection.
  // `side` is where the bar is docked. In the grid the bar is a fixed overlay at
  // the bottom: it has no grip and ignores the setting. `region` is the element
  // whose edges the bar can attach to.
  // `preview` makes a ghost for a drag: the bar as it will look at that side and
  // size. It acts on nothing and flashes nothing.
  let {
    forceShow = false,
    hidden = false,
    side: dockSide,
    region,
    preview,
  }: {
    forceShow?: boolean;
    hidden?: boolean;
    side: Side;
    region: () => HTMLElement | null;
    preview?: { side: Side; size: number };
  } = $props();
  const side = $derived(preview?.side ?? dockSide);

  // A ghost is inert: no tooltip listeners.
  const tip = (node: HTMLElement) => (preview ? undefined : tooltips(node));
  const dockable = $derived(view.mode !== "grid");
  const vertical = $derived(isVertical(side));
  const orientation = $derived(vertical ? "vertical" : "horizontal");

  // Size of the bar across its axis. A bar that leaves its slot during a drag
  // measures 0, so the last real size is kept. The other orientation is not on
  // screen, so its size is a constant: a row is one button high, a column is one
  // 44 px button wide. Both include the padding and the border.
  const ROW_SIZE = 51;
  const COLUMN_SIZE = 53;
  let ownSize = 0;
  let offsetH = $state(0);
  let offsetW = $state(0);
  const ds = $derived(!preview && dockable ? dock.state("actionBar") : DOCK_IDLE);
  const moving = $derived(dock.animating("actionBar"));
  $effect(() => {
    const size = vertical ? offsetW : offsetH;
    if (!preview && dockable && !moving && size) ownSize = size;
  });
  // A column has no room for a tag name: a short form stands for it.
  function abbreviation(name: string): string {
    const words = name.trim().split(/\s+/);
    const letters =
      words.length > 1 ? words.slice(0, 2).map((w) => Array.from(w)[0]) : Array.from(words[0]).slice(0, 2);
    return letters.join("").toUpperCase();
  }
  function thicknessFor(at: Side): number {
    const column = isVertical(at);
    if (column === vertical && ownSize) return ownSize;
    return column ? COLUMN_SIZE : ROW_SIZE;
  }

  // The item the actions apply to (mirrors the keyboard path, which acts on the
  // focused item / current selection).
  const focused = $derived(session.focused);
  const hasSelection = $derived(session.selectedIds.size > 0);

  // Configurable classification groups, in the user's chosen order, hidden ones
  // dropped (Settings → Bottom action bar). The selection/navigation groups
  // below are contextual and stay outside this list.
  const visibleBarItems = $derived(
    settings.bottomBarOrder.filter((id) => !settings.bottomBarHidden.includes(id)),
  );

  const scopedTags = $derived(
    tags.all.filter(
      (t) => t.scope === 2 || (catalog.media === "photos" ? t.scope === 0 : t.scope === 1),
    ),
  );

  // "Active" must reflect what's ACTUALLY applied — the current selection (when
  // one exists) or the single focused item otherwise — never a generic "this is
  // an action button" look. Only true when EVERY relevant item already carries
  // it, so partial/no application never falsely reads as active.
  const selectedItems = $derived(
    hasSelection ? session.filtered.filter((i) => session.selectedIds.has(i.id)) : [],
  );

  // Displayed rating: the common value when every selected item shares one,
  // the focused item's when there's no selection, and 0 (nothing lit) for a
  // mixed selection — never a misleading partial value. Star clicks toggle
  // against this same value, so display and action always agree.
  const rating = $derived(
    hasSelection
      ? selectedItems.length > 0 && selectedItems.every((i) => i.rating === selectedItems[0].rating)
        ? selectedItems[0].rating
        : 0
      : (focused?.rating ?? 0),
  );
  // Shared active state computed in ONE pass, read many times from the markup
  // (per flag button, per label swatch, per tag — in both the class binding and
  // the click handler). With a selection it's the common flag/label and the tag
  // intersection across every selected item (null/empty when mixed); otherwise
  // the single focused item's own state.
  const active = $derived.by(() => {
    if (hasSelection) {
      const items = selectedItems;
      if (items.length === 0)
        return { flag: null as number | null, label: null as string | null, tagIds: new Set<number>() };
      const first = items[0];
      return {
        flag: items.every((i) => i.flag === first.flag) ? first.flag : null,
        label: items.every((i) => i.label === first.label) ? first.label : null,
        tagIds: new Set<number>(first.tagIds.filter((t) => items.every((i) => i.tagIds.includes(t)))),
      };
    }
    return {
      flag: focused?.flag ?? null,
      label: focused?.label ?? null,
      tagIds: new Set<number>(focused?.tagIds ?? []),
    };
  });

  function labelActive(name: string): boolean {
    return active.label === name;
  }
  function tagActive(tagId: number): boolean {
    return active.tagIds.has(tagId);
  }
  // Pick/reject use the same selection-aware rule as labels/tags (and as the
  // desktop SelectionBar): the toggle reads whether EVERY relevant item
  // already has the flag, so both bars always issue the same command.
  function flagActive(f: number): boolean {
    return active.flag === f;
  }

  // Run a command, then release focus so a clicked button never swallows the
  // arrow keys used for grid navigation (mirrors the `blurring()` toolbar
  // helper). On touch this blur is a harmless no-op.
  function act(fn: () => void, id: string): (e: MouseEvent) => void {
    return (e) => {
      markPressed(id);
      fn();
      (e.currentTarget as HTMLElement).blur();
    };
  }

  // Flash the control that fired, from a tap or a key. The Web Animations API
  // restarts on every call, so a repeated press flashes again. Reduced motion
  // keeps the flash but drops the fade.
  let bar: HTMLElement;
  $effect(() => {
    const { id, n } = pressed;
    if (preview || !n) return;
    const calm = matchMedia("(prefers-reduced-motion: reduce)").matches;
    bar.querySelector(`[data-press="${CSS.escape(id)}"]`)?.animate(
      [{ backgroundColor: "rgba(255, 255, 255, 0.4)" }, {}],
      { duration: 200, easing: calm ? "steps(1, end)" : "ease-out" },
    );
  });

  function rate(n: number) {
    // Tapping the current rating clears it, Lightroom-style.
    runCommand((rating === n ? "rate.0" : `rate.${n}`) as CommandId);
  }

  function toggleAutoAdvance() {
    const enabled = !session.autoAdvanceActive;
    session.autoAdvancePref = false;
    settings.setFastCulling(enabled);
  }
</script>

{#if !preview}
  <DockGhost panel="actionBar">
    {#snippet ghost(ghostSide, size)}
      <TouchActionBar side={ghostSide} {region} {forceShow} {hidden} preview={{ side: ghostSide, size }} />
    {/snippet}
  </DockGhost>
{/if}
<div
  class="touchbar dock-panel {side}"
  bind:this={bar}
  bind:offsetHeight={offsetH}
  bind:offsetWidth={offsetW}
  use:tip
  class:vertical
  class:dockable
  class:forced={forceShow}
  class:preview={!!preview}
  class:disabled={!focused && !hasSelection}
  class:hidden
  class:docking={ds.docking}
  class:dock-moving={ds.moving}
  class:vacating={ds.vacating}
  style={preview ? `--bar-size: ${preview.size}px` : dockable && moving ? `--bar-size: ${thicknessFor(side)}px` : ""}
>
  {#if dockable}
    <div class="gripcell">
      <DockGrip panel="actionBar" still={!!preview} target={region} {thicknessFor} {orientation} />
    </div>
  {/if}
  <div class="items">
    {#if hasSelection}
      <div class="group select">
        <button class="btn" title="Clear selection" aria-label="Clear selection" data-press="select.none" onclick={act(() => session.clearSelection(), "select.none")}>
          <X size={20} />
        </button>
        {#if session.canSurvey && view.mode !== "survey"}
          <button class="btn task" title={shortcutHint("Review selection (Survey)", "view.survey")} aria-label="Review selection" data-press="view.survey" onclick={act(() => session.openSurvey(), "view.survey")}>
            <LayoutGrid size={18} />
            {#if !vertical}<span class="tasklabel">Review selection</span>{/if}
          </button>
        {/if}
      </div>
    {/if}
    {#if view.mode !== "grid"}
      <div class="group nav">
        <button class="btn" title="Previous" aria-label="Previous" data-press="nav.prev" onclick={act(() => runCommand("nav.prev"), "nav.prev")}>
          <ChevronLeft size={22} />
        </button>
        <button class="btn" title="Next" aria-label="Next" data-press="nav.next" onclick={act(() => runCommand("nav.next"), "nav.next")}>
          <ChevronRight size={22} />
        </button>
      </div>
    {/if}

    {#if view.mode === "viewer" || view.mode === "compare"}
      <div class="group recovery">
        <button class="btn" disabled={!session.canUndoRejection} aria-label="Undo last rejection" title={shortcutHint("Undo last rejection", "undo.rejection")} data-press="undo.rejection" onclick={act(() => void session.undoLastRejection(), "undo.rejection")}>
          <Undo2 size={18} />
        </button>
      </div>
    {/if}

    {#each visibleBarItems as itemId (itemId)}
      {#if itemId === "auto" && (view.mode === "viewer" || view.mode === "compare")}
        <div class="group recovery">
          <button class="btn task" class:active={session.autoAdvanceActive} disabled={session.capsLockActive} aria-label="Auto" aria-pressed={session.autoAdvanceActive} title={session.capsLockActive ? "Caps Lock enables keyboard auto-advance. Turn Caps Lock off to use Auto. Shift reverses keyboard auto-advance for one action." : "Auto: advance after classification. Shift reverses this setting for one action."} data-press="auto" onclick={act(toggleAutoAdvance, "auto")}>
            <span class="tasklabel">Auto</span>
          </button>
        </div>
      {:else if itemId === "moveCopy"}
        <div class="group file-actions">
          <button class="btn" title="Move or copy to folder" aria-label="Move or copy to folder" data-press="action.moveCopy" onclick={act(() => runCommand("action.moveCopy"), "action.moveCopy")}>
            <FolderInput size={18} />
          </button>
        </div>
      {:else if itemId === "flags"}
        <div class="group flags">
          <button
            class="btn reject"
            class:active-reject={flagActive(-1)}
            title={`${shortcutHint("Reject", "flag.reject")}. Click again to unflag.`}
            aria-label="Reject"
            data-press="flag.reject"
            onclick={act(() => runCommand(flagActive(-1) ? "flag.unflag" : "flag.reject"), "flag.reject")}
          >
            <X size={20} />
          </button>
          <button
            class="btn pick"
            class:active-pick={flagActive(1)}
            title={`${shortcutHint("Pick", "flag.pick")}. Click again to unflag.`}
            aria-label="Pick"
            data-press="flag.pick"
            onclick={act(() => runCommand(flagActive(1) ? "flag.unflag" : "flag.pick"), "flag.pick")}
          >
            <Check size={20} />
          </button>
        </div>
      {:else if itemId === "rating"}
        <div class="group stars">
          {#each [1, 2, 3, 4, 5] as n (n)}
            <button
              class="btn star"
              class:on={rating >= n}
              title={`Rate ${n}`}
              aria-label={`Rate ${n}`}
              data-press={`rate.${n}`}
              onclick={act(() => rate(n), `rate.${n}`)}
            >
              <Star size={18} fill={rating >= n ? "currentColor" : "none"} />
            </button>
          {/each}
        </div>
      {:else if itemId === "labels"}
        <div class="group labels">
          {#each Object.entries(LABEL_COLORS) as [name, color] (name)}
            <button
              class="btn swatchbtn"
              class:active={labelActive(name)}
              title={`${formatColorLabel(name)} label`}
              aria-label={`${formatColorLabel(name)} label`}
              style="--c: {color}"
              data-press={`label.${name.toLowerCase()}`}
              onclick={act(() => session.setLabel(labelActive(name) ? null : name), `label.${name.toLowerCase()}`)}
            >
              <span class="swatch"></span>
            </button>
          {/each}
        </div>
      {:else if itemId === "tags"}
        {#if scopedTags.length > 0}
          <div class="group tags">
            {#each scopedTags as tag (tag.id)}
              <button
                class="btn tagbtn"
                class:active={tagActive(tag.id)}
                title={`Toggle tag: ${tag.name}`}
                aria-label={`Toggle tag: ${tag.name}`}
                style="--c: {tag.color ?? '#888'}"
                data-press={`tag.${tag.id}`}
                onclick={act(() => session.toggleTag(tag.id), `tag.${tag.id}`)}
              >
                <span class="tagdot"></span>
                {#if vertical}
                  <span class="tagabbr">{abbreviation(tag.name)}</span>
                {:else}
                  <span class="taglabel">{tag.name}</span>
                {/if}
              </button>
            {/each}
          </div>
        {/if}
      {:else if itemId === "clear"}
        <div class="group clear">
          <button
            class="btn"
            title="Clear all classification (rating, flag, label, tags)"
            aria-label="Clear all classification"
            data-press="clear"
            onclick={act(() => session.clearClassification(), "clear")}
          >
            <Eraser size={20} />
          </button>
        </div>
      {/if}
    {/each}
  </div>
</div>

<style>
  /* Hidden by default; a coarse (touch) pointer or the desktop opt-in toggle
     (`.forced`) reveals it. Layout lives here so both paths share it. */
  .touchbar {
    --grip-zone: 14px;
    position: relative;
    display: none;
    box-sizing: border-box;
    flex: none;
    align-items: center;
    gap: 8px;
    padding: 4px calc(10px + var(--safe-right)) calc(4px + var(--safe-bottom))
      calc(10px + var(--safe-left));
    background: var(--surface);
    border-top: 1px solid var(--border-strong);
  }

  /* Only a bar with a grip needs the zone for it. The grid overlay bar has none. */
  .touchbar.dockable.top,
  .touchbar.dockable.bottom {
    padding-inline: calc(6px + var(--safe-left)) var(--safe-right);
  }

  /* The bar comes first in the frame for top and left. */
  .touchbar.top,
  .touchbar.left {
    order: -1;
  }

  /* The border sits on the inner edge, the one that faces the view. */
  .touchbar.top {
    padding-bottom: 4px;
    border-top: none;
    border-bottom: 1px solid var(--border-strong);
  }

  .touchbar.left,
  .touchbar.right {
    flex-direction: column;
    align-items: stretch;
    gap: 0;
    padding: 4px 4px calc(4px + var(--safe-bottom));
    border-top: none;
  }

  .touchbar.dockable.left,
  .touchbar.dockable.right {
    padding-block: 6px var(--safe-bottom);
  }

  .touchbar.left {
    padding-left: calc(4px + var(--safe-left));
    border-right: 1px solid var(--border-strong);
  }

  .touchbar.right {
    padding-right: calc(4px + var(--safe-right));
    border-left: 1px solid var(--border-strong);
  }

  /* Keep the button groups together and centered, with the slack split to the
     two sides. `safe` falls back to start-alignment when the groups overflow a
     narrow screen, so the leading ones stay reachable while it scrolls. */
  .items {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: safe center;
    gap: 8px;
    overflow-x: auto;
    flex-wrap: nowrap;
    scrollbar-width: none;
  }

  /* The same room at both ends, so the groups centre on the whole bar. At the
     grip it is a margin: scrolled items stop before the grip. At the far end it
     is padding inside the scroller: scrolled items run to the bar's edge. */
  .dockable .items {
    margin-inline-start: var(--grip-zone);
    padding-inline-end: calc(6px + var(--grip-zone));
  }

  .dockable.vertical .items {
    margin-inline-start: 0;
    padding-inline-end: 0;
    margin-block-start: var(--grip-zone);
    padding-block-end: calc(6px + var(--grip-zone));
  }

  .vertical .items {
    flex-direction: column;
    align-items: center;
    overflow-x: hidden;
    overflow-y: auto;
  }

  .items :global(button:focus-visible) {
    outline-offset: -2px;
  }

  .items::-webkit-scrollbar {
    display: none;
  }

  .touchbar :global(.dock-grip) {
    color: #8a8a93;
  }

  /* The grip takes no flow space. It sits in the leading padding and the zone in front of
     the items, and its hit area is the whole cell. */
  .gripcell {
    position: absolute;
    top: 0;
    bottom: 0;
    left: var(--safe-left);
    z-index: 1;
    display: flex;
    width: calc(6px + var(--grip-zone));
  }

  .vertical .gripcell {
    top: 0;
    bottom: auto;
    left: 0;
    right: 0;
    width: auto;
    height: calc(6px + var(--grip-zone));
  }

  .touchbar .gripcell :global(.dock-grip) {
    width: 100%;
    height: 100%;
  }

  /* On a drag, and on the way back after a cancel. The slot has an explicit size
     then, because a size can only move from one number to another. */
  .touchbar.dock-moving {
    height: var(--bar-size);
    transition:
      height var(--dock-motion),
      width var(--dock-motion),
      opacity var(--dock-motion),
      padding var(--dock-motion),
      border-width var(--dock-motion);
  }

  .touchbar.vertical.dock-moving {
    width: var(--bar-size);
    min-width: 0;
    height: auto;
  }

  .touchbar.docking {
    opacity: 0.5;
  }

  /* The preview is at another side: the bar closes its slot and the view takes
     the room. The node stays mounted, because the grip in it holds the pointer
     capture of the drag. */
  .touchbar.docking.vacating {
    height: 0;
    opacity: 0;
    overflow: hidden;
    border-width: 0;
    padding-block: 0;
  }

  .touchbar.docking.vacating.vertical {
    width: 0;
    height: auto;
    min-width: 0;
    padding-inline: 0;
  }

  @media (pointer: coarse) {
    .touchbar {
      --grip-zone: 28px;
      display: flex;
    }
  }

  .touchbar.forced {
    display: flex;
  }

  .touchbar.hidden {
    display: none;
  }

  /* A ghost fills the box that DockGhost gives it. */
  .touchbar.preview {
    display: flex;
    width: 100%;
    height: 100%;
  }

  .touchbar.disabled .group:not(.recovery) {
    opacity: 0.45;
    pointer-events: none;
  }

  .group {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
  }

  .group + .group {
    padding-left: 10px;
    border-left: 1px solid var(--border);
  }

  .vertical .group {
    flex-direction: column;
    align-items: center;
  }

  .vertical .group + .group {
    padding-left: 0;
    padding-top: 10px;
    border-left: none;
    border-top: 1px solid var(--border);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 44px;
    min-height: 42px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--control);
    color: #e8e8e8;
    padding: 0;
    touch-action: manipulation;
  }

  /* In a column every button is one 44 px square-ish cell, centred. */
  .vertical .btn {
    width: 44px;
    min-width: 44px;
    min-height: 36px;
  }

  .vertical .star,
  .vertical .swatchbtn {
    min-height: 30px;
  }

  @media (pointer: coarse) {
    .vertical .btn,
    .vertical .star,
    .vertical .swatchbtn {
      min-height: 42px;
    }
  }

  .btn:active {
    background: var(--hover);
  }

  @media (hover: hover) {
    .btn:hover:not(:disabled) {
      background: var(--hover);
    }
  }

  .btn:disabled { opacity: 0.4; }
  .btn.active { color: var(--accent); border-color: var(--accent); background: var(--accent-fill); }

  .task {
    gap: 5px;
    padding: 0 9px;
    white-space: nowrap;
    font-family: inherit;
    font-size: 12px;
  }

  /* In a column a task shows its icon only, except Auto, which is a word. The button keeps its title. */
  .vertical .task {
    gap: 0;
    padding: 0;
  }

  .star {
    min-width: 40px;
    border: none;
    background: none;
    color: #5a5a63;
  }

  .star.on {
    color: #e0c34f;
  }

  /* Pick/reject read as a green check / red X even when inactive, matching the
     thumbnail badges and the desktop selection bar; "active" adds the tinted
     highlight. */
  .pick {
    color: #6be675;
  }

  .reject {
    color: var(--danger);
  }

  .active-pick {
    border-color: #6be675;
    background: rgba(107, 230, 117, 0.14);
  }

  .active-reject {
    border-color: var(--danger);
    background: rgba(255, 107, 107, 0.14);
  }

  .swatch {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: var(--c);
    opacity: 0.75;
  }

  .swatchbtn.active .swatch {
    opacity: 1;
    outline: 2px solid #fff;
    outline-offset: 1px;
  }

  .tagbtn {
    min-width: auto;
    gap: 5px;
    padding: 0 10px;
    color: #bbb;
  }

  .tagbtn.active {
    background: var(--accent-fill);
    color: #fff;
    border-color: transparent;
  }

  .tagdot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    background: var(--c);
    flex: none;
  }

  .vertical .tagbtn {
    gap: 4px;
    padding: 0;
  }

  .tagabbr {
    font-size: 12px;
    font-weight: 600;
    line-height: 1;
  }

  .taglabel {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-size: 12px;
    white-space: nowrap;
  }
</style>
