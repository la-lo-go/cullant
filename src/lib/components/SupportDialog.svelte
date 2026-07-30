<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { backdropDismiss } from "../backdrop";
  import { FREE_WAYS, MONEY_WAYS, type SupportChannel } from "../support";
  import ArrowUpRight from "@lucide/svelte/icons/arrow-up-right";
  import Heart from "@lucide/svelte/icons/heart";
  import X from "@lucide/svelte/icons/x";

  let { onclose }: { onclose: () => void } = $props();

  const dismiss = backdropDismiss(() => onclose());

  let panel = $state<HTMLDivElement | null>(null);

  $effect(() => {
    panel?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") onclose();
  }
</script>

{#snippet way(c: SupportChannel)}
  <button class="way" onclick={() => void openUrl(c.url)}>
    <c.icon size={16} />
    <span class="text">
      <span class="label">{c.label}</span>
      <span class="note">{c.note}</span>
    </span>
    <ArrowUpRight size={13} />
  </button>
{/snippet}

<div class="backdrop" {...dismiss} role="presentation">
  <div
    class="dialog"
    bind:this={panel}
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    aria-label="Support Cullant"
    tabindex="-1"
  >
    <header>
      <Heart size={16} />
      <h2>Support Cullant</h2>
      <button class="close-x" onclick={onclose} aria-label="Close" title="Close">
        <X size={18} />
      </button>
    </header>

    <div class="content">
      <p class="intro">
        Cullant is free and open source. No ads, no account, no subscription, and it
        never sends your photos or anything about them anywhere. If it saves you time,
        here is how you can help it keep going.
      </p>

      <span class="head">Costs nothing</span>
      {#each FREE_WAYS as c (c.id)}
        {@render way(c)}
      {/each}

      <span class="head">With money</span>
      {#each MONEY_WAYS as c (c.id)}
        {@render way(c)}
      {/each}

      <p class="foot">Every link opens in your browser.</p>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(0, 0, 0, 0.55);
    padding: var(--dialog-edge-margin);
    padding-top: calc(var(--inset-top) + var(--dialog-edge-margin));
    padding-bottom: calc(var(--inset-bottom) + var(--dialog-edge-margin));
  }

  .dialog {
    outline: none;
    display: flex;
    flex-direction: column;
    width: 440px;
    max-width: 100%;
    max-height: 100%;
    background: #232329;
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.5);
  }

  header {
    display: flex;
    align-items: center;
    gap: 9px;
    padding: 12px 8px 12px 16px;
    border-bottom: 1px solid var(--border);
  }

  header :global(svg:first-child) {
    color: #ff7597;
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  .close-x {
    margin-left: auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 32px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    opacity: 0.6;
    cursor: pointer;
  }

  .close-x:hover {
    opacity: 1;
    background: var(--surface);
  }

  .content {
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 5px;
    padding: 14px 16px 16px;
  }

  .intro {
    margin: 0 2px 8px;
    font-size: 12.5px;
    line-height: 1.55;
    opacity: 0.75;
  }

  .head {
    margin: 8px 2px 2px;
    font-size: 10.5px;
    font-weight: 700;
    letter-spacing: 0.07em;
    text-transform: uppercase;
    opacity: 0.6;
  }

  .way {
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 9px 11px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    color: inherit;
    text-align: left;
    cursor: pointer;
  }

  .way:hover {
    border-color: var(--border-strong);
    background: var(--surface-2);
  }

  .way :global(svg:first-child) {
    flex: none;
    opacity: 0.8;
  }

  .way :global(svg:last-child) {
    flex: none;
    margin-left: auto;
    opacity: 0.45;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .label {
    font-size: 12.5px;
  }

  .note {
    font-size: 11px;
    line-height: 1.35;
    opacity: 0.6;
  }

  .foot {
    margin: 10px 2px 0;
    font-size: 11px;
    opacity: 0.5;
  }

  @media (max-width: 600px) {
    .backdrop {
      padding: 0;
    }

    .dialog {
      width: 100%;
      height: 100%;
      max-height: none;
      border: 0;
      border-radius: 0;
      padding-top: var(--inset-top);
    }
  }
</style>
