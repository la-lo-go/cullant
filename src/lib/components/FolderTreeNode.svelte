<script lang="ts">
  import { session } from "../stores/session.svelte";
  import FolderTreeNode from "./FolderTreeNode.svelte";
  import Folder from "@lucide/svelte/icons/folder";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import type { TreeNode } from "./folderTree";

  let { node, depth }: { node: TreeNode; depth: number } = $props();

  let expanded = $state(true);
  const hasChildren = $derived(node.children.size > 0);
  const children = $derived([...node.children.values()]);
</script>

<!-- The chevron is a real <button> sibling of the node button, overlaid on the
     node's leading spacer: nesting an interactive element inside a <button> is
     invalid HTML and leaves the chevron keyboard-unreachable. -->
<div class="row">
  <button
    class="node"
    class:active={session.folderFilter === node.path}
    style="padding-left: {depth * 16 + 8}px"
    onclick={() => (session.folderFilter = node.path)}
  >
    <span class="chevron-spacer"></span>
    <Folder size={13} />
    <span class="name">{node.name}</span>
    <span class="count">{node.count}</span>
  </button>
  {#if hasChildren}
    <button
      class="chevron"
      style="left: {depth * 16 + 8}px"
      aria-label={expanded ? "Collapse" : "Expand"}
      aria-expanded={expanded}
      onclick={() => (expanded = !expanded)}
    >
      {#if expanded}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
    </button>
  {/if}
</div>
{#if hasChildren && expanded}
  {#each children as child (child.path)}
    <FolderTreeNode node={child} depth={depth + 1} />
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
