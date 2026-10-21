<script lang="ts">
  /**
   * A short list of exclusive choices drawn side by side, as one radio group.
   * An option shows its icon when it has one, and then its label is only the
   * accessible name and the tooltip.
   */
  import type Zap from "@lucide/svelte/icons/zap";
  import { tooltips } from "../tooltips";

  interface Option {
    value: string;
    label: string;
    icon?: typeof Zap;
    /** What the option does, in its tooltip. */
    hint?: string;
  }

  interface Props {
    label: string;
    options: Option[];
    value: string;
    onselect: (value: string, e: Event) => void;
  }

  const { label, options, value, onselect }: Props = $props();

  let buttons: HTMLButtonElement[] = [];

  /** Arrow keys move the selection with the focus, as a native radio group does. */
  function onKeydown(e: KeyboardEvent, index: number) {
    const dir = { ArrowRight: 1, ArrowDown: 1, ArrowLeft: -1, ArrowUp: -1 }[e.key];
    if (!dir) return;
    e.preventDefault();
    const next = (index + dir + options.length) % options.length;
    buttons[next].focus();
    onselect(options[next].value, e);
  }
</script>

<div class="segments" role="radiogroup" aria-label={label} use:tooltips>
  {#each options as o, i (o.value)}
    {@const selected = o.value === value}
    <button
      bind:this={buttons[i]}
      type="button"
      role="radio"
      class:selected
      class:icon={!!o.icon}
      aria-checked={selected}
      aria-label={o.icon ? o.label : undefined}
      title={o.hint ?? (o.icon ? o.label : undefined)}
      tabindex={selected ? 0 : -1}
      onclick={(e) => onselect(o.value, e)}
      onkeydown={(e) => onKeydown(e, i)}
    >
      {#if o.icon}
        {@const Icon = o.icon}
        <Icon size={14} />
      {:else}
        {o.label}
      {/if}
    </button>
  {/each}
</div>

<style>
  .segments {
    flex: none;
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border);
    border-radius: 7px;
    background: var(--surface);
  }

  button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 22px;
    padding: 0 9px;
    border: 0;
    border-radius: 5px;
    background: none;
    color: inherit;
    font-size: 11.5px;
    opacity: 0.7;
    cursor: pointer;
  }

  button.icon {
    padding: 0;
  }

  button:hover {
    background: var(--hover);
    opacity: 1;
  }

  button.selected {
    background: color-mix(in srgb, var(--accent) 24%, transparent);
    color: var(--accent);
    opacity: 1;
  }

  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  @media (pointer: coarse) {
    button {
      min-width: 34px;
      height: 28px;
    }
  }
</style>
