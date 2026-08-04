<script lang="ts">
  import { session } from "../stores/session.svelte";
  import { backdropDismiss } from "../backdrop";

  let { groupId }: { groupId: number } = $props();

  const dismiss = backdropDismiss(() => (session.recoupleDialogFor = null));

  let panel = $state<HTMLDivElement | null>(null);

  // Focus the panel so Escape lands here (and stops) instead of the global keymap.
  $effect(() => {
    panel?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") session.recoupleDialogFor = null;
  }
</script>

<div
  class="backdrop"
  {...dismiss}
  onkeydown={(e) => e.key === "Escape" && (session.recoupleDialogFor = null)}
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
    <h2>Recouple RAW+JPEG pair</h2>
    <p>
      The two files may have diverged while decoupled. Which state should the
      reunited pair keep?
    </p>
    <div class="options">
      <button onclick={() => session.recouple(groupId, "raw")}>
        Use RAW's state
      </button>
      <button onclick={() => session.recouple(groupId, "jpeg")}>
        Use JPEG's state
      </button>
      <button onclick={() => session.recouple(groupId, "latest")}>
        Use the most recent edit
      </button>
      <button onclick={() => session.recouple(groupId, "none")}>
        Keep each as-is
      </button>
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
    width: 380px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    outline: none;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
  }

  p {
    font-size: 13px;
    opacity: 0.7;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
  }

  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 8px 10px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }
</style>
