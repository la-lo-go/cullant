<script lang="ts">
  import { session } from "$lib/stores/session.svelte";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { tags } from "$lib/stores/tags.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { runCommand } from "$lib/keyboard/dispatcher.svelte";
  import type { CommandId } from "$lib/keyboard/keymap";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Flag from "@lucide/svelte/icons/flag";
  import Ban from "@lucide/svelte/icons/ban";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import X from "@lucide/svelte/icons/x";

  // On touch devices the bar shows itself (coarse pointer). On desktop it is
  // opt-in: the parent flips `forceShow` from a toolbar toggle. `hidden` lets
  // the parent suppress the bar in the grid until there's a selection.
  let { forceShow = false, hidden = false }: { forceShow?: boolean; hidden?: boolean } =
    $props();

  // The item the actions apply to (mirrors the keyboard path, which acts on the
  // focused item / current selection).
  const focused = $derived(session.focused);
  const rating = $derived(focused?.rating ?? 0);
  const flag = $derived(focused?.flag ?? 0);
  const hasSelection = $derived(session.selectedIds.size > 0);

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };
  const labelCommands: Record<string, CommandId> = {
    Red: "label.red",
    Yellow: "label.yellow",
    Green: "label.green",
    Blue: "label.blue",
    Purple: "label.purple",
  };

  // Tags applicable to the current media type (same derivation as SelectionBar/FiltersPanel).
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
  function labelActive(name: string): boolean {
    if (hasSelection) return selectedItems.length > 0 && selectedItems.every((i) => i.label === name);
    return focused?.label === name;
  }
  function tagActive(tagId: number): boolean {
    if (hasSelection) return selectedItems.length > 0 && selectedItems.every((i) => i.tagIds.includes(tagId));
    return focused?.tagIds.includes(tagId) ?? false;
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

  <div class="group flags">
    <button class="btn danger" title="Queue delete" aria-label="Queue delete" onclick={act(() => runCommand("delete.pair"))}>
      <Trash2 size={20} />
    </button>
    <button
      class="btn"
      class:active-pick={flag === 1}
      title="Pick"
      aria-label="Pick"
      onclick={act(() => runCommand(flag === 1 ? "flag.unflag" : "flag.pick"))}
    >
      <Flag size={20} />
    </button>
  </div>

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

  <div class="group labels">
    {#each Object.entries(labelColors) as [name, color] (name)}
      <button
        class="btn swatchbtn"
        class:active={labelActive(name)}
        title={`${name} label`}
        aria-label={`${name} label`}
        style="--c: {color}"
        onclick={act(() => runCommand(labelCommands[name]))}
      >
        <span class="swatch"></span>
      </button>
    {/each}
  </div>

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

  <div class="group actions">
    <button
      class="btn"
      class:active-reject={flag === -1}
      title="Reject"
      aria-label="Reject"
      onclick={act(() => runCommand(flag === -1 ? "flag.unflag" : "flag.reject"))}
    >
      <Ban size={20} />
    </button>
    <button class="btn commit" title="Review & commit" aria-label="Review & commit" onclick={act(() => runCommand("commit.open"))}>
      <CheckCheck size={20} />
    </button>
  </div>
</div>

<style>
  /* Hidden by default; a coarse (touch) pointer or the desktop opt-in toggle
     (`.forced`) reveals it. Layout lives here so both paths share it. */
  .touchbar {
    display: none;
    align-items: center;
    /* Center the button groups; "safe" falls back to start when the content
       overflows so the leading items stay reachable while scrolling. */
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

  /* Always on where touch is the primary pointer. */
  @media (pointer: coarse) {
    .touchbar {
      display: flex;
    }
  }

  /* Desktop opt-in via the toolbar toggle. */
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

  .star {
    min-width: 40px;
    border: none;
    background: none;
    color: #5a5a63;
  }

  .star.on {
    color: #e0c34f;
  }

  .active-pick {
    color: #6be675;
    border-color: #6be675;
  }

  .active-reject {
    color: #ff6b6b;
    border-color: #ff6b6b;
  }

  .btn.danger:active {
    color: #ff6b6b;
  }

  .btn.commit {
    color: var(--accent);
    border-color: var(--border-strong);
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
