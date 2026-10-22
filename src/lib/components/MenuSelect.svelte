<script lang="ts">
  /**
   * A drop-down for a list of values, drawn with the app's own menu instead of
   * the native `<select>`. At rest it is only the current value and a chevron.
   */
  import ContextMenu from "./ContextMenu.svelte";
  import type { MenuNode } from "../menu";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  interface Props {
    label: string;
    options: { value: string; label: string }[];
    value: string;
    disabled?: boolean;
    onselect: (value: string, e: Event) => void;
  }

  const { label, options, value, disabled = false, onselect }: Props = $props();

  let button = $state<HTMLButtonElement | null>(null);
  let menu = $state<{ x: number; y: number } | null>(null);
  let viaKeyboard = false;

  const current = $derived(options.find((o) => o.value === value)?.label ?? value);

  const items = $derived<MenuNode[]>(
    options.map((o) => ({
      kind: "item",
      label: o.label,
      checked: o.value === value,
      run: () => onselect(o.value, new Event("select")),
    })),
  );

  function open(e: Event) {
    const r = button?.getBoundingClientRect();
    if (!r) return;
    viaKeyboard = !(e instanceof MouseEvent) || e.detail === 0;
    menu = { x: Math.round(r.right), y: Math.round(r.bottom + 4) };
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== "ArrowDown") return;
    e.preventDefault();
    open(e);
  }

  function close() {
    menu = null;
    if (viaKeyboard) button?.focus();
  }
</script>

<button
  bind:this={button}
  type="button"
  class="select"
  aria-label={label}
  aria-haspopup="menu"
  aria-expanded={menu !== null}
  {disabled}
  onclick={open}
  onkeydown={onKeydown}
>
  <span class="value">{current}</span>
  <ChevronDown size={12} />
</button>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} alignEnd {items} onclose={close} />
{/if}

<style>
  .select {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    max-width: 52%;
    padding: 3px 6px;
    border: 0;
    border-radius: 6px;
    background: none;
    color: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }

  .select:hover:not(:disabled) {
    background: var(--hover);
  }

  .select:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .select:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  .value {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .select :global(svg) {
    flex: none;
    opacity: 0.55;
  }

  @media (pointer: coarse) {
    .select {
      min-height: 28px;
    }
  }
</style>
