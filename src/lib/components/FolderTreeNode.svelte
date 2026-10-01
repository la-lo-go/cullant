<script lang="ts">
  import { folders } from "../stores/folders.svelte";
  import { folderContextMenu } from "../folderContextMenu";
  import FolderTreeNode from "./FolderTreeNode.svelte";
  import Folder from "@lucide/svelte/icons/folder";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import type { TreeNode } from "./folderTree";

  let { node, depth, onmenu, onselect, menuPath = null }: {
    node: TreeNode;
    depth: number;
    onmenu: (path: string, x: number, y: number) => void;
    onselect: (path: string, event: MouseEvent) => void;
    menuPath?: string | null;
  } = $props();

  const expanded = $derived(!folders.collapsed.has(node.path));
  const ignored = $derived(folders.ignoredBy(node.path) !== undefined);
  const included = $derived(!ignored && !folders.allSelected && folders.includes(node.path));
  const hasChildren = $derived(node.children.size > 0);
  const children = $derived([...node.children.values()]);

  function select(e: MouseEvent) {
    const button = e.currentTarget as HTMLButtonElement;
    if (ignored) {
      const bounds = button.getBoundingClientRect();
      onmenu(node.path, bounds.left, bounds.bottom);
    } else {
      onselect(node.path, e);
    }
    button.blur();
  }
</script>

<!-- The chevron is a real <button> sibling of the node button, overlaid on the
     node's leading spacer: nesting an interactive element inside a <button> is
     invalid HTML and leaves the chevron keyboard-unreachable. -->
<div class="row">
  <button
    class="node"
    class:active={included && folders.scope[node.path] === true}
    class:included={included && folders.scope[node.path] !== true}
    class:excluded={ignored || (!folders.allSelected && !included)}
    aria-pressed={!ignored && folders.includes(node.path)}
    class:ignored
    class:menu-target={menuPath === node.path}
    title={ignored ? `${node.path} (ignored)` : node.path}
    style="padding-left: {depth * 16 + 8}px"
    onclick={select}
    use:folderContextMenu={(x, y) => onmenu(node.path, x, y)}
  >
    <span class="chevron-spacer"></span>
    <Folder size={13} />
    <span class="name">{node.name}</span>
    <span class="count">{node.count} {node.count === 1 ? "file" : "files"}</span>
  </button>
  {#if hasChildren}
    <button
      class="chevron"
      style="left: {depth * 16 + 8}px"
      aria-label={expanded ? "Collapse" : "Expand"}
      aria-expanded={expanded}
      onclick={(e) => { folders.toggleCollapsed(node.path); e.currentTarget.blur(); }}
    >
      {#if expanded}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
    </button>
  {/if}
</div>
{#if hasChildren && expanded}
  {#each children as child (child.path)}
    <FolderTreeNode node={child} depth={depth + 1} {onmenu} {onselect} {menuPath} />
  {/each}
{/if}

<style>
  .row {
    position: relative;
  }

  .node {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    /* Right padding keeps the count clear of the overlay scrollbar (an 8px band
       pinned to the panel's right edge); the inline padding-left below overrides
       the left value per depth. */
    padding: 5px 14px 5px 8px;
    border: none;
    background: transparent;
    color: inherit;
    font-family: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }

  .node:hover {
    background: var(--hover);
  }

  .node.active {
    background: var(--accent-fill);
    color: var(--accent);
  }

  .node.included {
    background: color-mix(in srgb, var(--accent-fill) 40%, transparent);
    color: color-mix(in srgb, var(--accent) 55%, #ddd);
  }

  .node.excluded { color: #777; }
  .node.ignored .name { text-decoration: line-through; }

  .node.menu-target {
    background: var(--hover);
    color: var(--accent);
  }

  .chevron-spacer {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    flex: none;
  }

  /* Overlaid on the node's leading spacer (left set inline per depth). */
  .chevron {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 14px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    opacity: 0.6;
  }

  .chevron:hover {
    opacity: 1;
  }

  .name {
    flex: 1;
    /* Allow the flex item to shrink below its content width so the label
       ellipsizes instead of forcing the row wider than the panel. */
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    opacity: 0.5;
    font-size: 11px;
  }
</style>
