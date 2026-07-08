<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import FolderTreeNode from "./FolderTreeNode.svelte";
  import { buildFolderTree, collectFolderPaths } from "./folderTree";
  import Images from "@lucide/svelte/icons/images";

  const tree = $derived(buildFolderTree(catalog.items));
  const children = $derived([...tree.children.values()]);

  // If the selected folder disappeared (rescan, mirror-mode change removing
  // its only file, etc.) fall back to "All" instead of silently showing an
  // empty grid.
  $effect(() => {
    if (session.folderFilter === null) return;
    if (!collectFolderPaths(tree).has(session.folderFilter)) {
      session.folderFilter = null;
    }
  });
</script>

{#if children.length > 0}
  <aside class="tree">
    <div class="header">Folders</div>
    <button class="node root" class:active={session.folderFilter === null} onclick={() => (session.folderFilter = null)}>
      <Images size={13} />
      <span class="name">All</span>
      <span class="count">{tree.count}</span>
    </button>
    {#each children as child (child.path)}
      <FolderTreeNode node={child} depth={0} />
    {/each}
  </aside>
{/if}

<style>
  .tree {
    flex: none;
    width: 210px;
    overflow-y: auto;
    background: #1e1e23;
    border-right: 1px solid #2e2e36;
  }

  .header {
    padding: 8px 10px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.5;
  }

  .node.root {
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

  .node.root:hover {
    background: #26262c;
  }

  .node.root.active {
    background: #2f3a5c;
    color: #a9c0ff;
  }

  .name {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .count {
    opacity: 0.5;
    font-size: 11px;
  }
</style>
