<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import FolderTreeNode from "./FolderTreeNode.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
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

  // Drag-to-resize the panel. Dragging the right edge past COLLAPSE_AT hides
  // the panel altogether (a peek arrow in +page.svelte brings it back).
  const MIN_W = 160;
  const MAX_W = 480;
  const COLLAPSE_AT = 120;

  let aside = $state<HTMLElement | null>(null);
  let resizing = $state(false);

  // Scroll metrics feeding the overlay scrollbar (the native bar is hidden so
  // it doesn't steal 8px of panel width from the folder labels).
  let scroller = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let viewportH = $state(0);
  let contentH = $state(0);

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onResizeMove(e: PointerEvent) {
    if (!resizing || !aside) return;
    const w = e.clientX - aside.getBoundingClientRect().left;
    if (w < COLLAPSE_AT) {
      // Collapse but keep the last usable width for when it reopens.
      resizing = false;
      session.folderTreeVisible = false;
      return;
    }
    session.folderTreeWidth = Math.min(MAX_W, Math.max(MIN_W, w));
  }

  function endResize(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    session.setFolderTreeWidth(session.folderTreeWidth);
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      // capture may already be gone
    }
  }
</script>

{#if children.length > 0}
  <aside class="tree" bind:this={aside} style="--tree-w: {session.folderTreeWidth}px">
    <div
      class="scroll"
      id="folder-tree-scroll"
      bind:this={scroller}
      bind:clientHeight={viewportH}
      onscroll={() => scroller && (scrollTop = scroller.scrollTop)}
    >
      <div class="inner" bind:clientHeight={contentH}>
        <div class="header">Folders</div>
        <button class="node root" class:active={session.folderFilter === null} onclick={() => (session.folderFilter = null)}>
          <Images size={13} />
          <span class="name">All</span>
          <span class="count">{tree.count}</span>
        </button>
        {#each children as child (child.path)}
          <FolderTreeNode node={child} depth={0} />
        {/each}
      </div>
    </div>
    <OverlayScrollbar
      orientation="vertical"
      viewport={viewportH}
      content={contentH}
      position={scrollTop}
      controls="folder-tree-scroll"
      onSeek={(pos) => {
        if (scroller) scroller.scrollTop = pos;
      }}
    />
    <!-- Right-edge resize handle (hidden on narrow screens where the tree floats). -->
    <div
      class="resize-handle"
      class:resizing
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize folder panel"
      onpointerdown={startResize}
      onpointermove={onResizeMove}
      onpointerup={endResize}
      onpointercancel={endResize}
    ></div>
  </aside>
{/if}

<style>
  .tree {
    position: relative;
    display: flex;
    flex-direction: column;
    flex: none;
    width: var(--tree-w, 210px);
    background: var(--surface);
    border-right: 1px solid var(--border);
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    /* overflow-y: auto forces overflow-x to compute to auto (spec), so long
       folder names would produce a horizontal scrollbar. Clip instead — node
       labels already ellipsize. */
    overflow-x: hidden;
    /* Native bar hidden: a classic scrollbar would carve 8px out of the panel
       width. OverlayScrollbar floats over the content instead. */
    scrollbar-width: none;
  }

  .scroll::-webkit-scrollbar {
    display: none;
  }

  /* Grabbable strip straddling the right edge; widens the hit area without a
     visible chrome until hovered/dragged. */
  .resize-handle {
    position: absolute;
    top: 0;
    bottom: 0;
    right: -3px;
    width: 7px;
    cursor: col-resize;
    z-index: 25;
    touch-action: none;
  }

  .resize-handle::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 3px;
    width: 2px;
    background: transparent;
    transition: background 0.12s;
  }

  .resize-handle:hover::after,
  .resize-handle.resizing::after {
    background: var(--accent);
  }

  /* On narrow screens the tree floats over the grid instead of squeezing it
     into one column; toggle it off with the same toolbar/keyboard control. */
  @media (max-width: 720px) {
    .tree {
      position: absolute;
      top: 0;
      bottom: 0;
      left: 0;
      z-index: 20;
      width: min(75%, 240px);
      box-shadow: 4px 0 18px rgba(0, 0, 0, 0.55);
    }

    .resize-handle {
      display: none;
    }
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
    background: var(--hover);
  }

  .node.root.active {
    background: var(--accent-fill);
    color: var(--accent);
  }

  .name {
    flex: 1;
    /* Allow the flex item to shrink below its content width so ellipsis kicks
       in instead of forcing the row wider. */
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
