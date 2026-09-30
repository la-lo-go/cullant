<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import { folders, folderContains } from "../stores/folders.svelte";
  import { folderContextMenu } from "../folderContextMenu";
  import ContextMenu from "./ContextMenu.svelte";
  import type { MenuNode } from "../menu";
  import FolderTreeNode from "./FolderTreeNode.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
  import { buildFolderTree, collectFolderPaths, visibleFolderPaths } from "./folderTree";
  import Images from "@lucide/svelte/icons/images";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Eye from "@lucide/svelte/icons/eye";

  const tree = $derived(buildFolderTree(catalog.items, (path) => folders.ignoredBy(path) !== undefined));
  const children = $derived([...tree.children.values()]);
  const visiblePaths = $derived(visibleFolderPaths(tree, folders.collapsed));
  let menu = $state<{ path: string; x: number; y: number; items: MenuNode[] } | null>(null);

  function showAll() {
    session.folderFilter = null;
    session.clampFocus();
  }

  function selectFolder(path: string, event: MouseEvent) {
    folders.select(path, event, visiblePaths);
    session.clampFocus();
  }

  async function restoreAll() {
    if (await folders.showAll()) showAll();
  }

  function openAllMenu(x: number, y: number) {
    if (folders.ignored.length === 0) return;
    menu = { path: "", x, y, items: [
      { kind: "header", label: "All" },
      { kind: "item", label: "Show all folders again", icon: Eye, disabled: folders.saving, run: () => void restoreAll() },
    ] };
  }

  function openMenu(path: string, x: number, y: number) {
    const ignoredBy = folders.ignoredBy(path);
    const inherited = ignoredBy !== undefined && ignoredBy !== path;
    menu = { path, x, y, items: [
      { kind: "header", label: path },
      {
        kind: "item", label: inherited ? `Ignored by ${ignoredBy}` : ignoredBy ? "Show folder again" : "Ignore folder",
        icon: ignoredBy ? Eye : EyeOff,
        disabled: inherited || folders.saving,
        run: () => void folders.setIgnored(path, !ignoredBy),
      },
      { kind: "sep" },
      { kind: "item", label: "Expand branch", run: () => {
        for (const folder of folders.collapsed) if (folderContains(path, folder)) folders.collapsed.delete(folder);
      } },
      { kind: "item", label: "Collapse branch", run: () => folders.collapsed.add(path) },
    ] };
  }

  // Wait for the catalog and saved scope before removing missing folder paths.
  $effect(() => {
    if (catalog.preloading || catalog.scanning || session.restoring) return;
    folders.reconcileScope(collectFolderPaths(tree));
  });

  // Drag-to-resize the panel. Dragging the right edge past COLLAPSE_AT hides
  // the panel altogether (a peek arrow in +page.svelte brings it back).
  const MIN_W = 160;
  const MAX_W = 480;
  const COLLAPSE_AT = 120;

  let aside = $state<HTMLElement | null>(null);
  let resizing = $state(false);
  // True while the drag is past the collapse threshold: the tree clamps to its
  // minimum and dims, but the hide only commits on pointer release — dragging
  // back out cancels it (standard resize-to-hide behavior).
  let pendingCollapse = $state(false);

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
    // Past the threshold only flags a pending collapse (tree stays at MIN_W,
    // dimmed) instead of committing it, so dragging back out cancels the hide.
    pendingCollapse = w < COLLAPSE_AT;
    session.folderTreeWidth = Math.min(MAX_W, Math.max(MIN_W, w));
  }

  function endResize(e: PointerEvent) {
    if (!resizing) return;
    resizing = false;
    if (pendingCollapse) {
      pendingCollapse = false;
      session.folderTreeVisible = false;
    } else {
      session.setFolderTreeWidth(session.folderTreeWidth);
    }
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      // capture may already be gone
    }
  }
</script>

{#if children.length > 0}
  <aside class="tree" class:pending-collapse={pendingCollapse} bind:this={aside} style="--tree-w: {session.folderTreeWidth}px">
    <div
      class="scroll"
      id="folder-tree-scroll"
      bind:this={scroller}
      bind:clientHeight={viewportH}
      onscroll={() => scroller && (scrollTop = scroller.scrollTop)}
    >
      <div class="inner" bind:clientHeight={contentH}>
        <div class="header">
          <span>Folders</span>
        </div>
        <button class="node root" class:active={folders.allSelected} aria-pressed={folders.allSelected} class:menu-target={menu?.path === ""}
          use:folderContextMenu={openAllMenu} onclick={(e) => { showAll(); e.currentTarget.blur(); }}>
          <Images size={13} />
          <span class="name">All</span>
          <span class="count">{tree.count}</span>
        </button>
        {#each children as child (child.path)}
          <FolderTreeNode node={child} depth={0} onmenu={openMenu} onselect={selectFolder} menuPath={menu?.path ?? null} />
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
    <!-- Hide tab on the panel's inner (right) edge, pointing left to collapse.
         Overlaid (absolute) so it costs no layout width; sits above the resize
         handle and overlay scrollbar (higher z-index) so it stays clickable. -->
    <button
      class="tree-hide"
      title="Hide folder tree (D)"
      aria-label="Hide folder tree"
      onclick={() => (session.folderTreeVisible = false)}
    >
      <ChevronLeft size={16} />
    </button>
  </aside>
{/if}

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
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

  /* Dragged past the collapse threshold: a subtle dim signals that releasing
     now will hide the tree (dragging back out cancels). */
  .tree.pending-collapse {
    opacity: 0.6;
    transition: opacity 0.1s;
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

  /* Right-edge hide tab (same 18x44 size, surface fill, accent chevron as
     .tree-peek). Anchored to the panel's inner right edge and layered above the
     resize handle (z-index 25) and overlay scrollbar (z-index 26) so its click
     target wins. */
  .tree-hide {
    position: absolute;
    top: 50%;
    right: 0;
    transform: translateY(-50%);
    z-index: 27;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 44px;
    padding: 0;
    border: 1px solid var(--border);
    border-right: none;
    border-radius: 6px 0 0 6px;
    background: var(--surface);
    color: var(--accent);
    cursor: pointer;
  }

  .tree-hide:hover {
    background: var(--hover);
    border-color: var(--accent);
  }

  .header {
    position: sticky;
    top: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 36px;
    box-sizing: border-box;
    background: var(--surface);
    color: #888;
    padding: 8px 10px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  .node.root {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    /* Extra right padding keeps the count clear of the overlay scrollbar
       (an 8px band pinned to the panel's right edge) instead of sitting flush
       under the thumb. The full-width row highlight is unaffected. */
    padding: 5px 14px 5px 8px;
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

  .node.root.menu-target {
    background: var(--hover);
    color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
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
