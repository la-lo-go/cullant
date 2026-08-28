<script lang="ts">
  import {
    COMMANDS,
    effectiveBindings,
    loadOverrides,
    type CommandCategory,
    type CommandId,
  } from "../keyboard/keymap";
  import X from "@lucide/svelte/icons/x";

  let { onclose }: { onclose: () => void } = $props();

  const CATEGORY_ORDER: CommandCategory[] = [
    "Navigation",
    "Rating & flags",
    "Labels",
    "Views",
    "Zoom",
    "Pairs",
    "Actions",
    "UI",
  ];

  // Reverse the effective binding map (key -> command) into command -> keys, so
  // the sheet always reflects the user's current remaps. A key maps to exactly
  // one command (later COMMANDS win), matching real dispatch.
  const keysByCommand = (() => {
    const map = new Map<CommandId, string[]>();
    for (const [key, id] of effectiveBindings(loadOverrides())) {
      const arr = map.get(id) ?? [];
      arr.push(key);
      map.set(id, arr);
    }
    return map;
  })();

  const sections = CATEGORY_ORDER.map((category) => ({
    category,
    rows: COMMANDS.filter((c) => c.category === category).map((c) => ({
      title: c.title,
      bindings: keysByCommand.get(c.id) ?? [],
    })),
  })).filter((s) => s.rows.length > 0);

  const PRETTY_KEY: Record<string, string> = {
    arrowright: "→",
    arrowleft: "←",
    arrowup: "↑",
    arrowdown: "↓",
    space: "Space",
    escape: "Esc",
    enter: "Enter",
    delete: "Del",
    home: "Home",
    end: "End",
    "-": "−",
    "+": "+",
    "=": "=",
  };

  const MODS: [string, string][] = [
    ["ctrl+", "Ctrl"],
    ["alt+", "Alt"],
    ["shift+", "Shift"],
  ];

  function prettyKey(k: string): string {
    return PRETTY_KEY[k] ?? (k.length === 1 ? k.toUpperCase() : k);
  }

  function keyChips(binding: string): string[] {
    const chips: string[] = [];
    let rest = binding;
    let peeled = true;
    while (peeled) {
      peeled = false;
      for (const [prefix, label] of MODS) {
        if (rest.startsWith(prefix)) {
          chips.push(label);
          rest = rest.slice(prefix.length);
          peeled = true;
        }
      }
    }
    // An empty remainder means the base key itself is "+" (e.g. "ctrl++").
    chips.push(prettyKey(rest === "" ? "+" : rest));
    return chips;
  }

  // Close on Escape from within the panel, then stop propagation so the key
  // never reaches the global keymap behind the overlay.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") onclose();
    e.stopPropagation();
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
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    aria-label="Keyboard shortcuts"
    tabindex="-1"
  >
    <header>
      <h2>Keyboard shortcuts</h2>
      <button class="close-x" onclick={onclose} aria-label="Close" title="Close">
        <X size={18} />
      </button>
    </header>
    <div class="sections">
      {#each sections as section}
        <section>
          <h3>{section.category}</h3>
          <div class="rows">
            {#each section.rows as row}
              <span class="title">{row.title}</span>
              <span class="keys">
                {#if row.bindings.length === 0}
                  <span class="unbound">—</span>
                {:else}
                  {#each row.bindings as binding, bi}
                    {#if bi > 0}<span class="sep">/</span>{/if}
                    <span class="combo">
                      {#each keyChips(binding) as chip, ci}
                        {#if ci > 0}<span class="plus">+</span>{/if}
                        <kbd>{chip}</kbd>
                      {/each}
                    </span>
                  {/each}
                {/if}
              </span>
            {/each}
          </div>
        </section>
      {/each}
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
    padding: var(--inset-top) var(--inset-right) var(--inset-bottom) var(--inset-left);
    box-sizing: border-box;
  }

  .dialog {
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 16px 20px;
    width: 720px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    max-height: 84vh;
    display: flex;
    flex-direction: column;
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  h2 {
    margin: 0;
    font-size: 16px;
    flex: 1;
  }

  .sections {
    overflow-y: auto;
    /* Pack sections into responsive columns so the long list stays compact. */
    columns: 2;
    column-gap: 28px;
  }

  section {
    break-inside: avoid;
    margin-bottom: 14px;
  }

  h3 {
    margin: 0 0 6px;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    opacity: 0.55;
  }

  .rows {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 5px 12px;
    align-items: center;
  }

  .title {
    font-size: 13px;
  }

  .keys {
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
    justify-content: flex-end;
  }

  .combo {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .sep,
  .plus {
    font-size: 11px;
    opacity: 0.5;
  }

  .unbound {
    font-size: 12px;
    opacity: 0.4;
  }

  kbd {
    font-family: Consolas, monospace;
    font-size: 11px;
    line-height: 1;
    min-width: 16px;
    text-align: center;
    background: var(--control);
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    padding: 4px 6px;
    color: #e8e8e8;
  }

  /* Corner dismiss: borderless icon button, ≥40px hit area for touch. */
  .close-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    flex: none;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
  }

  .close-x:hover {
    background: var(--hover);
    opacity: 1;
  }
</style>
