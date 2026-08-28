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
   *
   * Every panel is positioned `fixed` and placed by script, submenus included.
   * An absolutely-positioned submenu would be clipped by its own scroller: a
   * list with `overflow-y: auto` computes `overflow-x` to `auto` as well, so the
   * part of the submenu that hangs outside the parent panel simply disappears.
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

  /** Place the root panel, and do it again whenever the anchor moves: a
   *  right-click elsewhere reuses this component instance rather than mounting
   *  a new one, so a mount-time placement would leave the menu at the old
   *  cursor. Any open submenu belongs to the previous menu and is dropped. */
  $effect(() => {
    const el = rootEl;
    void x;
    void y;
    void items;
    if (!el) return;
    activePath = [];
    place(el, x, y);
  });

  /** Put a fixed panel at a point, flipping past it rather than sliding along
   *  it when it runs out of room — a menu that covers its own anchor is worse
   *  than one that opens the other way. */
  function place(el: HTMLElement, atX: number, atY: number, flipLeftTo?: number) {
    el.style.left = "0px";
    el.style.top = "0px";
    const r = el.getBoundingClientRect();
    const left =
      atX + r.width > window.innerWidth - EDGE
        ? Math.max(EDGE, (flipLeftTo ?? atX) - r.width)
        : atX;
    const top =
      atY + r.height > window.innerHeight - EDGE
        ? Math.max(EDGE, window.innerHeight - EDGE - r.height)
        : atY;
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
  }

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

  /** Open a submenu off its parent row: to the right normally, flipped to the
   *  row's left edge when the right side has no room. */
  function placeSub(el: HTMLElement) {
    const row = el.parentElement?.querySelector<HTMLElement>(".row");
    if (!row) return;
    const rr = row.getBoundingClientRect();
    // The panel's own padding, so the first child row lines up with the parent.
    place(el, rr.right, rr.top - 5, rr.left);
  }

</script>

{#snippet level(nodes: MenuNode[], path: number[])}
  <!-- Submenus are placed against their row's viewport rect, so scrolling a long
       panel would leave one floating beside nothing. Close it instead. -->
  <ul class="menu" onscroll={() => (activePath = activePath.slice(0, path.length + 1))}>
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
  oncontextmenu={(e) => e.preventDefault()}
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

  /* Fixed, not absolute: the parent panel scrolls, and a scroller clips both
     axes, so an absolute submenu is cut off at the panel's edge. */
  .sub {
    position: fixed;
    z-index: 62;
  }
</style>
