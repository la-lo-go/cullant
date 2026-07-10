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

<button
  class="node"
  class:active={session.folderFilter === node.path}
  style="padding-left: {depth * 16 + 8}px"
  onclick={() => (session.folderFilter = node.path)}
>
  {#if hasChildren}
    <span
      class="chevron"
      role="button"
      tabindex="-1"
      onclick={(e) => {
        e.stopPropagation();
        expanded = !expanded;
      }}
      onkeydown={(e) => {
        if (e.key !== "Enter" && e.key !== " ") return;
        e.stopPropagation();
        e.preventDefault();
        expanded = !expanded;
      }}
    >
      {#if expanded}<ChevronDown size={12} />{:else}<ChevronRight size={12} />{/if}
    </span>
  {:else}
    <span class="chevron-spacer"></span>
  {/if}
  <Folder size={13} />
  <span class="name">{node.name}</span>
  <span class="count">{node.count}</span>
</button>
{#if hasChildren && expanded}
  {#each children as child (child.path)}
    <FolderTreeNode node={child} depth={depth + 1} />
  {/each}
{/if}

<style>
  .node {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    padding: 5px 8px;
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

  .chevron,
  .chevron-spacer {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    flex: none;
    opacity: 0.6;
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
