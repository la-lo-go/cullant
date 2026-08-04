<script lang="ts">
  /**
   * A cursor-anchored menu over a `MenuNode` tree.
   *
   * Placement is bespoke rather than `clampToViewport`: that helper anchors a
   * panel under a toolbar button and nudges it with a transform, while a context
   * menu opens at a point and has to flip past the cursor when it runs out of
   * room. Dismissal still goes through the shared `backdropDismiss`.
   *
   * Navigation state is one `activePath` of indexes into the tree. A submenu is
   * open exactly when the path passes through it, so hovering, arrowing and
   * clicking all move the same variable and can never disagree about what is
   * open.
   */
  import { backdropDismiss } from "../backdrop";
  import { isSelectable, type MenuNode } from "../menu";
  import Check from "@lucide/svelte/icons/check";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  interface Props {
    x: number;
    y: number;
    items: MenuNode[];
    onclose: () => void;
  }

  const { x, y, items, onclose }: Props = $props();

  /** Gap kept from the viewport edge, matching the dialog edge margin. */
  const EDGE = 8;

  let activePath = $state<number[]>([]);
  let rootEl = $state<HTMLDivElement | null>(null);

  const dismiss = backdropDismiss(() => onclose());

  $effect(() => {
    rootEl?.focus();
  });

  function nodesAt(path: number[]): MenuNode[] {
    let nodes = items;
    for (const i of path) {
      const node = nodes[i];
      if (node?.kind !== "submenu") return [];
      nodes = node.children;
    }
    return nodes;
  }

  function nodeAt(path: number[]): MenuNode | undefined {
    if (path.length === 0) return undefined;
    return nodesAt(path.slice(0, -1))[path[path.length - 1]];
  }

  /** Next selectable index from `from` in `dir`, wrapping at the ends. */
  function step(nodes: MenuNode[], from: number, dir: number): number {
    if (nodes.length === 0) return -1;
    for (let n = 1; n <= nodes.length; n++) {
      const i = (from + dir * n + nodes.length * n) % nodes.length;
      if (isSelectable(nodes[i])) return i;
    }
    return -1;
  }

  function activate(path: number[]) {
    const node = nodeAt(path);
    if (!node || !isSelectable(node)) return;
    if (node.kind === "submenu") {
      const first = step(node.children, -1, 1);
      if (first >= 0) activePath = [...path, first];
      return;
    }
    // Close first: a `run` that opens a dialog must not land behind this menu.
    onclose();
    node.run();
  }

  function onKeydown(e: KeyboardEvent) {
    // The grid's keymap is global; without this, arrowing the menu would also
    // move the focused photo underneath it.
    e.stopPropagation();
    const parent = activePath.slice(0, -1);
    const nodes = nodesAt(parent);
    const current = activePath.length === 0 ? -1 : activePath[activePath.length - 1];

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        onclose();
        return;
      case "ArrowDown":
      case "ArrowUp": {
        e.preventDefault();
        const next = step(nodes, current, e.key === "ArrowDown" ? 1 : -1);
        if (next >= 0) activePath = [...parent, next];
        return;
      }
      case "ArrowRight": {
        const node = nodeAt(activePath);
        if (node?.kind === "submenu") {
          e.preventDefault();
          activate(activePath);
        }
        return;
      }
      case "ArrowLeft":
        if (activePath.length > 1) {
          e.preventDefault();
          activePath = parent;
        }
        return;
      case "Enter":
      case " ":
        e.preventDefault();
        activate(activePath);
        return;
      case "Home":
      case "End": {
        e.preventDefault();
        const next = step(nodes, e.key === "Home" ? -1 : 0, e.key === "Home" ? 1 : -1);
        if (next >= 0) activePath = [...parent, next];
        return;
      }
    }
  }

  /** Keep the root panel inside the viewport, flipping past the cursor rather
   *  than sliding along it — a menu that covers its own anchor is worse than one
   *  that opens the other way. */
  function placeRoot(el: HTMLElement) {
    const r = el.getBoundingClientRect();
    const left = x + r.width > window.innerWidth - EDGE ? Math.max(EDGE, x - r.width) : x;
    const top =
      y + r.height > window.innerHeight - EDGE
        ? Math.max(EDGE, window.innerHeight - EDGE - r.height)
        : y;
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
  }

  /** Flip a submenu to the parent's other side when it would leave the viewport,
   *  and lift it when it would run off the bottom. */
  function placeSub(el: HTMLElement) {
    const r = el.getBoundingClientRect();
    if (r.right > window.innerWidth - EDGE) el.classList.add("flip-x");
    const over = r.bottom - (window.innerHeight - EDGE);
    if (over > 0) el.style.marginTop = `${-Math.min(over, Math.max(0, r.top - EDGE))}px`;
  }
</script>

{#snippet level(nodes: MenuNode[], path: number[])}
  <ul class="menu">
    {#each nodes as node, i (i)}
      {#if node.kind === "sep"}
        <li class="sep" role="separator"></li>
      {:else if node.kind === "header"}
        <li class="header">{node.label}</li>
      {:else}
        {@const active = activePath[path.length] === i}
        <li class="row-wrap">
          <button
            class="row"
            class:active
            class:checked={node.kind === "item" && node.checked}
            disabled={node.disabled}
            onpointerenter={() => (activePath = [...path, i])}
            onclick={() => activate([...path, i])}
          >
            <span class="ico">
              {#if node.icon}
                {@const Icon = node.icon}
                <Icon size={13} />
              {:else if node.kind === "item" && node.checked}
                <Check size={13} />
              {/if}
            </span>
            <span class="label">{node.label}</span>
            {#if node.kind === "submenu"}
              <ChevronRight size={13} class="chev" />
            {:else if node.hint}
              <span class="hint">{node.hint}</span>
            {/if}
          </button>

          {#if node.kind === "submenu" && active && !node.disabled}
            <div class="sub" use:placeSub>
              {@render level(node.children, [...path, i])}
            </div>
          {/if}
        </li>
      {/if}
    {/each}
  </ul>
{/snippet}

<div class="backdrop" role="presentation" {...dismiss}></div>

<div
  class="root"
  bind:this={rootEl}
  role="menu"
  tabindex="-1"
  aria-label="Context menu"
  onkeydown={onKeydown}
  use:placeRoot
>
  {@render level(items, [])}
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
  }

  .root {
    position: fixed;
    z-index: 61;
    outline: none;
  }

  .menu {
    list-style: none;
    margin: 0;
    padding: 4px;
    min-width: 190px;
    max-width: 320px;
    max-height: calc(100dvh - 24px);
    overflow-y: auto;
    scrollbar-width: none;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    font-size: 12px;
  }

  .menu::-webkit-scrollbar {
    display: none;
  }

  .row-wrap {
    position: relative;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 5px 8px;
    background: none;
    border: none;
    border-radius: 6px;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }

  .row.active {
    background: var(--hover);
  }

  .row:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .row.checked {
    color: var(--accent);
  }

  /* Reserve the icon column on every row so labels line up whether or not a
     row carries an icon or a check. */
  .ico {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    flex: none;
    color: #9aa0a0;
  }

  .row.checked .ico {
    color: var(--accent);
  }

  .label {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .hint {
    flex: none;
    color: #8a8a93;
    font-size: 11px;
  }

  .sep {
    height: 1px;
    margin: 4px 6px;
    background: var(--border);
  }

  .header {
    padding: 5px 8px 2px;
    color: #8a8a93;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .sub {
    position: absolute;
    top: -5px;
    left: 100%;
    z-index: 1;
  }

  .sub:global(.flip-x) {
    left: auto;
    right: 100%;
  }
</style>
