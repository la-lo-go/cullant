<script lang="ts">
  import { keymap } from "../keyboard/dispatcher.svelte";
  import { COMMANDS, DEFAULT_BINDINGS, normalizeKey } from "../keyboard/keymap";
  import X from "@lucide/svelte/icons/x";
  import { backdropDismiss } from "../backdrop";

  let { onclose }: { onclose: () => void } = $props();

  function currentKeys(id: (typeof COMMANDS)[number]["id"]): string {
    return (keymap.overrides[id] ?? DEFAULT_BINDINGS[id]).join(", ");
  }

  const dismiss = backdropDismiss(() => onclose());

  let panel = $state<HTMLDivElement | null>(null);

  // Focus the panel so keydowns land here first (and stop) instead of the
  // global keymap.
  $effect(() => {
    panel?.focus();
  });

  // With the panel focused, keydowns reach this handler first; it stops them
  // from bubbling to the global window dispatcher (so grid/loupe shortcuts
  // never fire behind the dialog). That also means it must capture the rebind
  // key itself, since dispatcher.svelte.ts's handleKeydown never sees it.
  function onKeydown(e: KeyboardEvent) {
    if (keymap.rebinding) {
      e.preventDefault();
      e.stopPropagation();
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
    bind:this={panel}
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    tabindex="-1"
  >
    <header>
      <h2>Keyboard shortcuts</h2>
      <button onclick={() => keymap.reset()}>Reset all</button>
      <button class="close-x" onclick={onclose} aria-label="Close" title="Close">
        <X size={18} />
      </button>
    </header>
    <p class="hint">
      Click a shortcut, then press the new key. Esc cancels. Hold Shift while rating to
      advance one photo (or to stay put when Caps Lock auto-advance is on).
    </p>
    <div class="list">
      {#each COMMANDS as cmd}
        <span class="title">{cmd.title}</span>
        <button
          class="key"
          class:waiting={keymap.rebinding === cmd.id}
          onclick={() => (keymap.rebinding = cmd.id)}
        >
          {keymap.rebinding === cmd.id ? "press a key…" : currentKeys(cmd.id)}
        </button>
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
    grid-template-columns: 1fr auto;
    gap: 4px 16px;
    overflow-y: auto;
    align-items: center;
  }

  .title {
    font-size: 13px;
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
