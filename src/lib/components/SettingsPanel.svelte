<script lang="ts">
  import type { Snippet } from "svelte";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import X from "@lucide/svelte/icons/x";
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";

  let {
    title,
    ariaLabel = title,
    onclose,
    onback,
    children,
    headerActions,
    panel = $bindable(null),
    modalOptions = {},
    pointerMode = false,
    touchPointer = false,
    onclick,
    onkeydown,
    onkeydowncapture,
    onfocusin,
    onpointerdowncapture,
    onpointermovecapture,
  }: {
    title: string;
    ariaLabel?: string;
    onclose: () => void;
    onback?: () => void;
    children: Snippet;
    headerActions?: Snippet;
    panel?: HTMLDivElement | null;
    modalOptions?: Parameters<typeof modalFocus>[1];
    pointerMode?: boolean;
    touchPointer?: boolean;
    onclick?: (event: MouseEvent) => void;
    onkeydown?: (event: KeyboardEvent) => void;
    onkeydowncapture?: (event: KeyboardEvent) => void;
    onfocusin?: (event: FocusEvent) => void;
    onpointerdowncapture?: (event: PointerEvent) => void;
    onpointermovecapture?: (event: PointerEvent) => void;
  } = $props();

  const dismiss = backdropDismiss(() => onclose());

  function handleKeydown(event: KeyboardEvent) {
    event.stopPropagation();
    if (onkeydown) onkeydown(event);
    else if (event.key === "Escape") (onback ?? onclose)();
  }
</script>

<div class="backdrop" {...dismiss} role="presentation">
  <div
    class="dialog settings-panel"
    class:pointer-mode={pointerMode}
    class:touch-pointer={touchPointer}
    bind:this={panel}
    use:modalFocus={modalOptions}
    {onclick}
    onkeydown={handleKeydown}
    {onkeydowncapture}
    {onfocusin}
    {onpointerdowncapture}
    {onpointermovecapture}
    role="dialog"
    aria-label={ariaLabel}
    tabindex="-1"
  >
    <header class="head">
      {#if onback}
        <button class="back" onclick={onback} aria-label="Back" title="Back">
          <ChevronLeft size={18} />
        </button>
      {/if}
      <h2>{title}</h2>
      {#if headerActions}{@render headerActions()}{/if}
      <button class="close-x" onclick={onclose} aria-label="Close settings" title="Close">
        <X size={18} />
      </button>
    </header>
    {@render children()}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
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
    width: 720px;
    max-width: 100%;
    max-height: 100%;
    background: #232329;
    border: 1px solid var(--border-strong);
    border-radius: 12px;
    box-shadow: 0 18px 50px rgba(0, 0, 0, 0.5);
  }

  .dialog :global(*:focus:not(:focus-visible)) { outline: none; }
  .dialog.pointer-mode :global(select:focus) {
    outline: none;
    border-color: var(--border-strong);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 8px 12px 16px;
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  h2 { margin: 0; font-size: 15px; font-weight: 600; }

  .back, .close-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 40px;
    height: 32px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    opacity: 0.6;
    cursor: pointer;
  }

  .close-x { margin-left: auto; }
  .back { margin-left: -6px; }

  @media (hover: hover) {
    .dialog:not(.touch-pointer) .back:hover,
    .dialog:not(.touch-pointer) .close-x:hover { opacity: 1; background: var(--hover); }
  }

  @media (max-width: 600px) { .dialog { width: 100%; } }
</style>
