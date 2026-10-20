<script lang="ts">
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";
  import X from "@lucide/svelte/icons/x";

  let {
    title,
    message,
    confirmLabel = "Confirm",
    altLabel,
    onalt,
    enterConfirms = true,
    initialFocus,
    choice = false,
    onconfirm,
    oncancel,
  }: {
    title: string;
    message: string;
    confirmLabel?: string;
    /** A second way forward, next to Cancel and the confirm button. */
    altLabel?: string;
    onalt?: () => void;
    /** False makes the confirm button click-only, for an action that replaces data. */
    enterConfirms?: boolean;
    /** Selector of the button that gets focus first. */
    initialFocus?: string;
    /** A choice between two safe options: confirm is the accent button and an X replaces Cancel. */
    choice?: boolean;
    onconfirm: () => void;
    oncancel: () => void;
  } = $props();

  const dismiss = backdropDismiss(() => oncancel());

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") oncancel();
    // Enter confirms the default action — unless a specific button has focus,
    // in which case let its own activation handle it (e.g. Cancel).
    else if (e.key === "Enter" && enterConfirms && !(e.target instanceof HTMLButtonElement)) onconfirm();
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
    use:modalFocus={{ initialFocus }}
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    tabindex="-1"
  >
    {#if choice}
      <button class="close-x" onclick={oncancel} aria-label="Close" title="Close"><X size={18} /></button>
    {/if}
    <h2>{title}</h2>
    <p>{message}</p>
    <div class="actions">
      {#if !choice}<button onclick={oncancel}>Cancel</button>{/if}
      {#if altLabel && onalt}
        <button class="alt" onclick={onalt}>{altLabel}</button>
      {/if}
      <button class="confirm" class:accent={choice} onclick={onconfirm}>{confirmLabel}</button>
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
    position: relative;
  }

  .close-x {
    position: absolute;
    top: 8px;
    right: 8px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    padding: 0;
    border: 0;
    background: none;
    opacity: 0.6;
  }

  .close-x:hover {
    opacity: 1;
  }

  h2 {
    margin: 0 36px 8px 0;
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

  button.confirm.accent {
    background: var(--accent-fill);
  }

  button.confirm.accent:hover {
    background: var(--accent-fill);
    filter: brightness(1.15);
  }
</style>
