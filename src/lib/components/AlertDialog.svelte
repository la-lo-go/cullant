<script lang="ts">
  let { title = "Notice", message, onclose }: { title?: string; message: string; onclose: () => void } =
    $props();

  let panel = $state<HTMLDivElement | null>(null);

  // Focus the panel so Escape/Enter land here (and stop) instead of the global keymap.
  $effect(() => {
    panel?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape" || e.key === "Enter") onclose();
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
    bind:this={panel}
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="alertdialog"
    aria-modal="true"
    tabindex="-1"
  >
    <h2>{title}</h2>
    <p>{message}</p>
    <div class="actions">
      <button class="ok" onclick={onclose}>OK</button>
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
    max-width: calc(100vw - 24px);
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

  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 12px;
  }

  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 8px 14px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }

  /* Default action, filled (no colored border). */
  button.ok {
    border-color: transparent;
    background: var(--accent-fill);
    color: #fff;
  }

  button.ok:hover {
    border-color: transparent;
    filter: brightness(1.1);
  }
</style>
