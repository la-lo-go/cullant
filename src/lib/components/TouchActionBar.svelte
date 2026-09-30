<script lang="ts">
  import { session } from "$lib/stores/session.svelte";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { tags } from "$lib/stores/tags.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { runCommand } from "$lib/keyboard/dispatcher.svelte";
  import { formatColorLabel } from "$lib/colorLabels";
  import type { CommandId } from "$lib/keyboard/keymap";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Star from "@lucide/svelte/icons/star";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Eraser from "@lucide/svelte/icons/eraser";
  import FolderInput from "@lucide/svelte/icons/folder-input";

  // On touch devices the bar shows itself (coarse pointer). On desktop it is
  // opt-in: the parent flips `forceShow` from a toolbar toggle. `hidden` lets
  // the parent suppress the bar in the grid until there's a selection.
  let { forceShow = false, hidden = false }: { forceShow?: boolean; hidden?: boolean } =
    $props();

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
  function act(fn: () => void): (e: MouseEvent) => void {
    return (e) => {
      fn();
      (e.currentTarget as HTMLElement).blur();
    };
  }

  function rate(n: number) {
    // Tapping the current rating clears it, Lightroom-style.
    runCommand((rating === n ? "rate.0" : `rate.${n}`) as CommandId);
  }
</script>

<div class="touchbar" class:forced={forceShow} class:disabled={!focused && !hasSelection} class:hidden>
  {#if hasSelection}
    <div class="group select">
      <button class="btn" title="Clear selection" aria-label="Clear selection" onclick={act(() => session.clearSelection())}>
        <X size={20} />
      </button>
    </div>
  {/if}
  {#if view.mode !== "grid"}
    <div class="group nav">
      <button class="btn" title="Previous" aria-label="Previous" onclick={act(() => runCommand("nav.prev"))}>
        <ChevronLeft size={22} />
      </button>
      <button class="btn" title="Next" aria-label="Next" onclick={act(() => runCommand("nav.next"))}>
        <ChevronRight size={22} />
      </button>
    </div>
  {/if}

  {#each visibleBarItems as itemId (itemId)}
    {#if itemId === "moveCopy"}
      <div class="group file-actions">
        <button class="btn task" title="Move or copy to folder" aria-label="Move or copy to folder" onclick={act(() => runCommand("action.moveCopy"))}>
          <FolderInput size={18} /> Move / Copy
        </button>
      </div>
    {:else if itemId === "flags"}
      <div class="group flags">
        <button
          class="btn reject"
          class:active-reject={flagActive(-1)}
          title="Reject (X). Click again to unflag."
          aria-label="Reject"
          onclick={act(() => runCommand(flagActive(-1) ? "flag.unflag" : "flag.reject"))}
        >
          <X size={20} />
        </button>
        <button
          class="btn pick"
          class:active-pick={flagActive(1)}
          title="Pick (P). Click again to unflag."
          aria-label="Pick"
          onclick={act(() => runCommand(flagActive(1) ? "flag.unflag" : "flag.pick"))}
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
            onclick={act(() => rate(n))}
          >
            <Star size={18} fill={rating >= n ? "currentColor" : "none"} />
          </button>
        {/each}
      </div>
    {:else if itemId === "labels"}
      <div class="group labels">
        {#each Object.entries(labelColors) as [name, color] (name)}
          <button
            class="btn swatchbtn"
            class:active={labelActive(name)}
            title={`${formatColorLabel(name)} label`}
            aria-label={`${formatColorLabel(name)} label`}
            style="--c: {color}"
            onclick={act(() => session.setLabel(labelActive(name) ? null : name))}
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
              title={tag.name}
              style="--c: {tag.color ?? '#888'}"
              onclick={act(() => session.toggleTag(tag.id))}
            >
              <span class="tagdot"></span>
              <span class="taglabel">{tag.name}</span>
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
          onclick={act(() => session.clearClassification())}
        >
          <Eraser size={20} />
        </button>
      </div>
    {/if}
  {/each}
</div>

<style>
  /* Hidden by default; a coarse (touch) pointer or the desktop opt-in toggle
     (`.forced`) reveals it. Layout lives here so both paths share it. */
  .touchbar {
    display: none;
    align-items: center;
    /* Keep the button groups together and centered, with the slack split to the
       two sides. `safe` falls back to start-alignment when the groups overflow a
       narrow screen, so the leading ones stay reachable while it scrolls. */
    justify-content: safe center;
    gap: 8px;
    overflow-x: auto;
    flex-wrap: nowrap;
    padding: 4px calc(10px + var(--safe-right)) calc(4px + var(--safe-bottom))
      calc(10px + var(--safe-left));
    background: var(--surface);
    border-top: 1px solid var(--border-strong);
    scrollbar-width: none;
  }

  @media (pointer: coarse) {
    .touchbar {
      display: flex;
    }
  }

  .touchbar.forced {
    display: flex;
  }

  .touchbar.hidden {
    display: none;
  }

  .touchbar::-webkit-scrollbar {
    display: none;
  }

  .touchbar.disabled {
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

  .btn:active {
    background: var(--hover);
  }

  .task {
    gap: 5px;
    padding: 0 9px;
    white-space: nowrap;
    font-family: inherit;
    font-size: 12px;
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
    color: #ff6b6b;
  }

  .active-pick {
    border-color: #6be675;
    background: rgba(107, 230, 117, 0.14);
  }

  .active-reject {
    border-color: #ff6b6b;
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

  .taglabel {
    font-size: 12px;
    white-space: nowrap;
  }
</style>
