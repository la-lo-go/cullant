<script lang="ts">
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

  let panel = $state<HTMLDivElement | null>(null);

  // Focus the panel so Escape lands here (and stops) instead of the global keymap.
  $effect(() => {
    panel?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") oncancel();
  }
</script>

<div
  class="backdrop"
  onclick={oncancel}
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
  }

  .dialog {
    background: #232329;
    border: 1px solid #3a3a42;
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
    gap: 8px;
    margin-top: 12px;
  }

  button {
    border-radius: 6px;
    border: 1px solid #3a3a42;
    padding: 8px 12px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: #2a2a30;
    cursor: pointer;
  }

  button:hover {
    border-color: #6b6bff;
  }

  button.confirm {
    border-color: #a04040;
  }

  button.confirm:hover {
    border-color: #ff6b6b;
  }
</style>
