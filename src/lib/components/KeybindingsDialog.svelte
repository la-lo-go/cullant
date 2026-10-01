<script lang="ts">
  import { keymap } from "../keyboard/dispatcher.svelte";
  import { COMMANDS, DEFAULT_BINDINGS, formatKey, isModifierKey, normalizeKey, type CommandId } from "../keyboard/keymap";
  import { formatColorLabel } from "../colorLabels";
  import Search from "@lucide/svelte/icons/search";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import X from "@lucide/svelte/icons/x";
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";

  let { onclose }: { onclose: () => void } = $props();

  function currentKeys(id: CommandId): string {
    return [...keymap.bindings].filter(([, command]) => command === id)
      .map(([key]) => formatKey(key)).join(", ") || "Unassigned";
  }

  function bindingsChanged(id: CommandId): boolean {
    const keys = [...keymap.bindings].filter(([, command]) => command === id).map(([key]) => key);
    const defaults = DEFAULT_BINDINGS[id];
    return keys.length !== defaults.length || keys.some((key) => !defaults.includes(key));
  }

  const modifiedCommands = $derived(new Set(COMMANDS.filter((cmd) => bindingsChanged(cmd.id)).map((cmd) => cmd.id)));

  function commandTitle(cmd: (typeof COMMANDS)[number]): string {
    if (!cmd.id.startsWith("label.")) return cmd.title;
    const color = cmd.title.replace(" label", "");
    return `${formatColorLabel(color)} label`;
  }

  let query = $state("");
  const visibleCommands = $derived(COMMANDS.filter((cmd) => {
    const text = `${commandTitle(cmd)} ${cmd.category} ${currentKeys(cmd.id)}`.toLowerCase();
    return query.trim().toLowerCase().split(/\s+/).every((word) => text.includes(word));
  }));

  const dismiss = backdropDismiss(() => onclose());

  // A shortcut waiting for its new key must not outlive the dialog. `rebinding`
  // is global state the window dispatcher also honours, so a dialog closed
  // (X, backdrop, Escape) while one row said "press a key…" left the next key
  // press anywhere in the app rebinding that command instead of running it.
  $effect(() => () => {
    keymap.rebinding = null;
  });

  // With the panel focused, keydowns reach this handler first; it stops them
  // from bubbling to the global window dispatcher (so grid/loupe shortcuts
  // never fire behind the dialog). That also means it must capture the rebind
  // key itself, since dispatcher.svelte.ts's handleKeydown never sees it.
  function onKeydown(e: KeyboardEvent) {
    if (keymap.rebinding) {
      e.preventDefault();
      e.stopPropagation();
      if (isModifierKey(e)) return;
      const key = normalizeKey(e);
      if (key !== "escape") keymap.rebind(keymap.rebinding, key);
      keymap.rebinding = null;
      return;
    }
    e.stopPropagation();
    if (e.key === "Escape") onclose();
  }
</script>

<div
  class="backdrop"
  {...dismiss}
  onkeydown={(e) => e.key === "Escape" && onclose()}
  role="presentation"
>
  <div
    class="dialog"
    use:modalFocus={{ isRecording: () => keymap.rebinding !== null }}
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    aria-label="Keyboard shortcuts"
    tabindex="-1"
  >
    <header>
      <h2>Keyboard shortcuts</h2>
      {#if modifiedCommands.size > 0}
        <button onclick={() => keymap.reset()}>Reset all</button>
      {/if}
      <button class="close-x" onclick={onclose} aria-label="Close" title="Close">
        <X size={18} />
      </button>
    </header>
    <p class="hint">
      Click a shortcut, then press the new key. Esc cancels. Hold Shift to reverse
      auto-advance for one classification. This also applies to Auto and Caps Lock.
    </p>
    <label class="find">
      <Search size={14} />
      <input type="search" aria-label="Search shortcuts" placeholder="Search commands or keys" bind:value={query} onfocus={() => (keymap.rebinding = null)} />
    </label>
    <div class="list">
      {#each visibleCommands as cmd (cmd.id)}
        <span class="title">{commandTitle(cmd)}</span>
        <button
          class="key"
          aria-label={`Change ${cmd.title} shortcut`}
          class:waiting={keymap.rebinding === cmd.id}
          onclick={() => (keymap.rebinding = cmd.id)}
        >
          {keymap.rebinding === cmd.id ? "press a key…" : currentKeys(cmd.id)}
        </button>
        {#if modifiedCommands.has(cmd.id) && keymap.overrides[cmd.id] !== undefined}
          <button
            class="reset"
            aria-label={`Reset ${cmd.title} shortcut`}
            title={`Reset ${cmd.title} shortcut`}
            onclick={() => keymap.resetCommand(cmd.id)}
          ><RotateCcw size={13} /></button>
        {:else}
          <span aria-hidden="true"></span>
        {/if}
      {:else}
        <p class="empty">No shortcuts match “{query}”.</p>
      {/each}
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    /* Keep the centered panel inside the safe area (system bars, cutout). */
    padding: var(--inset-top) var(--inset-right) var(--inset-bottom) var(--inset-left);
    box-sizing: border-box;
  }

  .dialog {
    box-sizing: border-box;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 16px 20px;
    width: 460px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    outline: none;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  h2 {
    margin: 0;
    font-size: 16px;
    flex: 1;
  }

  .hint {
    font-size: 12px;
    opacity: 0.6;
  }

  .list {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto auto;
    gap: 4px 8px;
    overflow-y: auto;
    align-items: center;
  }

  .title {
    font-size: 13px;
    min-width: 0;
    overflow-wrap: anywhere;
  }

  .key {
    font-family: Consolas, monospace;
    font-size: 12px;
    min-width: 90px;
    text-align: center;
    background: var(--control);
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    padding: 3px 8px;
    color: #e8e8e8;
    cursor: pointer;
  }

  .key.waiting {
    border-color: var(--accent);
    color: var(--accent);
  }

  .find {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 12px;
    padding: 6px 8px;
    background: var(--control);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
  }

  .find input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: inherit;
    font: inherit;
    outline: none;
  }

  .reset {
    display: inline-flex;
    justify-content: center;
    padding: 5px;
  }

  .empty {
    grid-column: 1 / -1;
    font-size: 12px;
  }

  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 4px 10px;
    font-size: 12px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
  }

  /* Corner dismiss: borderless icon button, ≥40px hit area for touch. */
  .close-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: inherit;
    opacity: 0.7;
  }

  .close-x:hover {
    background: var(--hover);
    opacity: 1;
  }
</style>
