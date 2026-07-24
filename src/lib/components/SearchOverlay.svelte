<script lang="ts">
  import { session } from "../stores/session.svelte";
  import { keymap } from "../keyboard/dispatcher.svelte";
  import { normalizeKey } from "../keyboard/keymap";
  import SearchIcon from "@lucide/svelte/icons/search";
  import X from "@lucide/svelte/icons/x";

  let inputEl = $state<HTMLInputElement | null>(null);

  // Focus the field on open, and select what is already there so a second
  // Ctrl+F retypes over the previous query instead of appending to it.
  $effect(() => {
    inputEl?.focus();
    inputEl?.select();
  });

  const matches = $derived(session.filtered.length);

  function close() {
    session.searchOpen = false;
  }

  function clear() {
    session.nameFilter = "";
    session.clampFocus();
    inputEl?.focus();
  }

  // The global dispatcher skips keydowns aimed at an <input>, so culling
  // shortcuts never fire while typing here — but that also means Escape and
  // Enter must be handled locally.
  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      close();
    } else if (e.key === "Enter") {
      if (session.filtered.length > 0) session.focusedIndex = 0;
      close();
    } else if (keymap.bindings.get(normalizeKey(e)) === "ui.search") {
      // Pressing the open shortcut again closes, as a find bar should. Resolved
      // through the keymap rather than hard-coded, so a remap still works.
      e.preventDefault();
      close();
    }
  }
</script>

<div class="backdrop" role="presentation" onclick={close}></div>

<div
  class="panel"
  role="dialog"
  aria-label="Search by file name"
  tabindex="-1"
  onkeydown={onKeydown}
>
  <SearchIcon size={15} />
  <input
    bind:this={inputEl}
    bind:value={session.nameFilter}
    type="text"
    placeholder="Search file names…"
    spellcheck="false"
    autocomplete="off"
    oninput={() => session.clampFocus()}
  />
  <span class="count" class:none={matches === 0}>
    {matches}
    {matches === 1 ? "match" : "matches"}
  </span>
  {#if session.nameFilter !== ""}
    <button class="icon" aria-label="Clear search" title="Clear" onclick={clear}>
      <X size={14} />
    </button>
  {/if}
  <button class="icon" aria-label="Close search" title="Close (Esc)" onclick={close}>
    <X size={15} />
  </button>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 40;
  }

  /* Not anchored to a toolbar button (there is none — this opens from the
     keyboard), so it floats near the top centre, clear of the safe-area inset. */
  .panel {
    position: fixed;
    top: calc(var(--inset-top) + 64px);
    left: 50%;
    transform: translateX(-50%);
    z-index: 41;
    display: flex;
    align-items: center;
    gap: 8px;
    width: 420px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2 - var(--inset-left) - var(--inset-right));
    padding: 8px 10px;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    color: #8a8a93;
    font-size: 12px;
  }

  input {
    flex: 1;
    min-width: 0;
    background: none;
    border: none;
    outline: none;
    color: #eee;
    font-family: inherit;
    font-size: 13px;
    padding: 2px 0;
  }

  input::placeholder {
    color: #6a6a72;
  }

  .count {
    flex: none;
    color: #8a8a93;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .count.none {
    color: #d08770;
  }

  .icon {
    flex: none;
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

  .icon:hover {
    color: #fff;
  }
</style>
