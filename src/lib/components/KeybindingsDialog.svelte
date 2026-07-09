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
  }

  .dialog {
    background: #232329;
    border: 1px solid #3a3a42;
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
    background: #2a2a30;
    border: 1px solid #3a3a42;
    border-radius: 5px;
    padding: 3px 8px;
    color: #e8e8e8;
    cursor: pointer;
  }

  .key.waiting {
    border-color: #6b6bff;
    color: #6bb2ff;
  }

  button {
    border-radius: 6px;
    border: 1px solid #3a3a42;
    padding: 4px 10px;
    font-size: 12px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: #2a2a30;
    cursor: pointer;
  }
</style>
