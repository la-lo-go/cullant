<script lang="ts">
  import { keymap } from "../keyboard/dispatcher.svelte";
  import { COMMANDS, DEFAULT_BINDINGS } from "../keyboard/keymap";

  let { onclose }: { onclose: () => void } = $props();

  function currentKeys(id: (typeof COMMANDS)[number]["id"]): string {
    return (keymap.overrides[id] ?? DEFAULT_BINDINGS[id]).join(", ");
  }
</script>

<div
  class="backdrop"
  onclick={onclose}
  onkeydown={(e) => e.key === "Escape" && onclose()}
  role="presentation"
>
  <div
    class="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    tabindex="-1"
  >
    <header>
      <h2>Keyboard shortcuts</h2>
      <button onclick={() => keymap.reset()}>Reset all</button>
      <button onclick={onclose}>Close</button>
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
    max-width: calc(100vw - 24px);
    max-height: 80vh;
    display: flex;
    flex-direction: column;
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
</style>
