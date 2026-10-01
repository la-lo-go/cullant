<script lang="ts">
  import { api } from "../api";
  import { session } from "../stores/session.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";

  let dest = $state("selects");
  let busy = $state(false);
  let error = $state("");
  let lastDest = $state<string | null>(null);

  $effect(() => {
    try {
      lastDest = localStorage.getItem("cullant.lastMoveDestination");
    } catch {
      lastDest = null;
    }
  });

  // Keep the destination a relative path under the project root. Drop empty,
  // "." and ".." segments (so "../../outside" cannot climb out) and any segment
  // with a colon (a Windows drive like "C:" is drive-relative, so PathBuf::join
  // on the backend would discard the project root and jump to that drive).
  const safeFolder = $derived(
    dest
      .split(/[\\/]+/)
      .filter((s) => s !== "" && s !== "." && s !== ".." && !s.includes(":"))
      .join("/"),
  );

  function moveTargets() {
    return session.targets(session.moveDialogTargets ?? undefined);
  }

  const targets = $derived(moveTargets());
  const photoCount = $derived(targets?.ids.length ?? 0);
  const fileCount = $derived(targets ? session.targetFileCount(targets) : 0);

  function close() {
    session.moveDialogOpen = false;
    session.moveDialogTargets = null;
  }

  const dismiss = backdropDismiss(close);

  async function queue(action: "move" | "copy") {
    if (busy) return;
    const targets = moveTargets();
    if (!targets || !safeFolder) return;
    const destination = safeFolder;
    busy = true;
    const generation = catalog.generation;
    error = "";
    try {
      await api.enqueueAction(targets, action, destination, "both");
      if (generation !== catalog.generation) return;
      await session.refreshPending();
      if (generation !== catalog.generation) return;
      try {
        localStorage.setItem("cullant.lastMoveDestination", destination);
      } catch {
        // A storage failure must not change the queued action result.
      }
      close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      void queue(e.ctrlKey ? "copy" : "move");
    }
    if (e.key === "Escape") close();
    e.stopPropagation();
  }
</script>

<div class="backdrop" {...dismiss} onkeydown={(e) => e.key === "Escape" && close()} role="presentation">
  <div
    class="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    aria-label="Queue move / copy"
    tabindex="-1"
    use:modalFocus={{ initialFocus: "input" }}
  >
    <h2>Queue move / copy</h2>
    <p class="targets">{photoCount} {catalog.media === "videos" ? (photoCount === 1 ? "video" : "videos") : (photoCount === 1 ? "photo" : "photos")} · {fileCount} {fileCount === 1 ? "file" : "files"}</p>
    <p class="hint">
      Destination subfolder inside the project (created when you execute reviewed changes).
      Enter = move · Ctrl+Enter = copy · Esc = cancel
    </p>
    <input aria-label="Destination subfolder" disabled={busy} bind:value={dest} onfocus={(e) => e.currentTarget.select()} placeholder="e.g. selects or deliver/client" />
    {#if lastDest}
      <button class="last-destination" disabled={busy} onclick={() => (dest = lastDest ?? dest)}>Use last destination: {lastDest}</button>
    {/if}
    {#if error}<p class="error">{error}</p>{/if}
    <div class="buttons">
      <button onclick={() => queue("move")} disabled={busy || !safeFolder || !targets}>Queue move</button>
      <button onclick={() => queue("copy")} disabled={busy || !safeFolder || !targets}>Queue copy</button>
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

  .targets {
    margin: 0;
    font-size: 13px;
  }

  .last-destination {
    text-align: left;
    overflow-wrap: anywhere;
    font-size: 12px;
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

  button:hover:not(:disabled) {
    border-color: var(--accent);
  }

  button:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .error {
    color: #ff6b6b;
    font-size: 13px;
    margin: 0;
  }
</style>
