<script lang="ts">
  import { update } from "../stores/update.svelte";
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";
  import X from "@lucide/svelte/icons/x";

  const dismiss = backdropDismiss(() => update.later());

  function onkeydown(event: KeyboardEvent) {
    event.stopPropagation();
    if (event.key === "Escape") update.later();
  }
</script>

{#if update.prompt}
  <div class="backdrop" role="presentation" {...dismiss}>
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Update available" tabindex="-1" use:modalFocus={{ initialFocus: "[data-action=update]" }} {onkeydown}>
      <header>
        <h2>Cullant {update.latest} is available</h2>
        <button class="close" aria-label="Close" onclick={() => update.later()}><X size={18} /></button>
      </header>
      <p>You have {update.current}.</p>
      <footer>
        <button data-action="skip" onclick={() => update.skip()}>Skip this version</button>
        <button class="primary" data-action="update" onclick={() => update.open()}>Update</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 110; display: flex; align-items: center; justify-content: center; background: #0008; padding: var(--dialog-edge-margin); padding-top: calc(var(--inset-top) + var(--dialog-edge-margin)); padding-bottom: calc(var(--inset-bottom) + var(--dialog-edge-margin)); }
  .dialog { outline: none; width: 380px; max-width: 100%; display: flex; flex-direction: column; background: #232329; border: 1px solid var(--border-strong); border-radius: 12px; box-shadow: 0 18px 50px #0008; }
  header, footer { display: flex; align-items: center; gap: 12px; padding: 14px 16px; }
  header { border-bottom: 1px solid var(--border); }
  h2 { flex: 1; margin: 0; font-size: 15px; }
  p { color: #bbb; font-size: 12px; line-height: 1.5; margin: 0; padding: 16px; }
  button { font: inherit; cursor: pointer; border: 1px solid var(--border); background: var(--control); color: inherit; padding: 7px 12px; border-radius: 6px; font-size: 12px; }
  .close { display: inline-flex; align-items: center; border: none; background: none; padding: 5px; }
  footer { justify-content: flex-end; border-top: 1px solid var(--border); }
  .primary { background: var(--accent-fill); color: #fff; }
</style>
