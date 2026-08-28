<script lang="ts">
  import { backdropDismiss } from "../backdrop";

  let {
    title,
    message,
    confirmLabel = "Confirm",
    onconfirm,
    oncancel,
  }: {
    title: string;
    message: string;
    confirmLabel?: string;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

  const dismiss = backdropDismiss(() => oncancel());

  let panel = $state<HTMLDivElement | null>(null);

  // Focus the panel so Escape lands here (and stops) instead of the global keymap.
  $effect(() => {
    panel?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") oncancel();
    // Enter confirms the default action — unless a specific button has focus,
    // in which case let its own activation handle it (e.g. Cancel).
    else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement)) onconfirm();
  }
</script>

<div
  class="backdrop"
  {...dismiss}
  onkeydown={(e) => e.key === "Escape" && oncancel()}
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
    <h2>{title}</h2>
    <p>{message}</p>
    <div class="actions">
      <button onclick={oncancel}>Cancel</button>
      <button class="confirm" onclick={onconfirm}>{confirmLabel}</button>
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
    /* Guaranteed gutter on every side: the larger of the safe-area inset (system
       bars / cutout) and the shared edge margin, so the panel never touches the
       screen edges on mobile. */
    padding: max(var(--inset-top), var(--dialog-edge-margin))
      max(var(--inset-right), var(--dialog-edge-margin))
      max(var(--inset-bottom), var(--dialog-edge-margin))
      max(var(--inset-left), var(--dialog-edge-margin));
    box-sizing: border-box;
  }

  .dialog {
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 16px 20px;
    width: 380px;
    max-width: 100%;
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
    gap: 8px;
    margin-top: 12px;
  }

  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 8px 12px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }

  button.confirm {
    border-color: transparent;
    background: #a04040;
    color: #fff;
  }

  button.confirm:hover {
    border-color: transparent;
    background: #b84a4a;
  }
</style>
