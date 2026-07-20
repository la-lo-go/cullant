<script lang="ts">
  import { api } from "../api";
  import { session } from "../stores/session.svelte";

  let dest = $state("selects");
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    input?.focus();
    input?.select();
  });

  function close() {
    session.moveDialogOpen = false;
  }

  async function queue(action: "move" | "copy") {
    const item = session.focused;
    const folder = dest.trim().replace(/^[\\/]+|[\\/]+$/g, "");
    if (!item || !folder) return;
    await api.enqueueAction(
      { ids: [item.id], asGroups: session.mirrorMode },
      action,
      folder,
      "both"
    );
    await session.refreshPending();
    close();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") queue(e.ctrlKey ? "copy" : "move");
    if (e.key === "Escape") close();
    e.stopPropagation();
  }
</script>

<div class="backdrop" onclick={close} onkeydown={(e) => e.key === "Escape" && close()} role="presentation">
  <div
    class="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    tabindex="-1"
  >
    <h2>Queue move / copy</h2>
    <p class="hint">
      Destination subfolder inside the project (created on commit).
      Enter = move · Ctrl+Enter = copy · Esc = cancel
    </p>
    <input bind:this={input} bind:value={dest} placeholder="e.g. selects or deliver/client" />
    <div class="buttons">
      <button onclick={() => queue("move")}>Queue move</button>
      <button onclick={() => queue("copy")}>Queue copy</button>
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
    width: 360px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  h2 {
    margin: 0;
    font-size: 15px;
  }

  .hint {
    font-size: 12px;
    opacity: 0.6;
    margin: 0;
  }

  input,
  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 7px 10px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
  }

  input:focus {
    border-color: var(--accent);
    outline: none;
  }

  .buttons {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
  }

  button {
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }
</style>
