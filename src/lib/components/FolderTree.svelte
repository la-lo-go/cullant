<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import { folders, folderContains } from "../stores/folders.svelte";
  import { api } from "../api";
  import { folderContextMenu } from "../folderContextMenu";
  import { shortcutHint } from "../keyboard/hints";
  import ContextMenu from "./ContextMenu.svelte";
  import type { MenuNode } from "../menu";
  import FolderTreeNode from "./FolderTreeNode.svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";
  import DockGrip from "./DockGrip.svelte";
  import DockGhost from "./DockGhost.svelte";
  import FolderTree from "./FolderTree.svelte";
  import { DOCK_IDLE, dock, NARROW, type Side } from "../stores/dock.svelte";
  import { buildFolderTree, collectFolderPaths, hasCollapsedBranch, visibleFolderPaths, type TreeNode } from "./folderTree";
  import Images from "@lucide/svelte/icons/images";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import FolderPlus from "@lucide/svelte/icons/folder-plus";
  import EyeOff from "@lucide/svelte/icons/eye-off";
  import Eye from "@lucide/svelte/icons/eye";

  // `preview` makes a ghost for a drag: the tree as it will look at that side and
  // size, scrolled like the real one. It reads the stores and writes nothing.
  // `previewScroll` is the scroll position of the real tree.
  const { preview, previewScroll = 0 }: { preview?: { side: Side; size: number }; previewScroll?: number } = $props();

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

  async function createProject(path: string) {
    const generation = catalog.generation;
    try {
      const projectPath = await api.folderProjectPath(path);
      if (generation !== catalog.generation) return;
      await catalog.open(projectPath);
    } catch (error) {
      if (generation === catalog.generation) catalog.error = `Could not create the folder project: ${error}`;
    }
  }

  function branchActionsFor(path: string): MenuNode[] {
    let node: TreeNode | undefined = tree;
    for (const segment of path.split("/")) node = node?.children.get(segment);
    const branchActions: MenuNode[] = [];
    if (node?.children.size) {
      if (hasCollapsedBranch(node, folders.collapsed)) branchActions.push({
        kind: "item", label: "Expand branch", run: () => {
          for (const folder of folders.collapsed) if (folderContains(path, folder)) folders.collapsed.delete(folder);
        },
      });
      if (!folders.collapsed.has(path)) branchActions.push({
        kind: "item", label: "Collapse branch", run: () => folders.collapsed.add(path),
      });
    }
    return branchActions;
  }

  function openMenu(path: string, x: number, y: number) {
    const ignoredBy = folders.ignoredBy(path);
    const inherited = ignoredBy !== undefined && ignoredBy !== path;
    const branchActions = branchActionsFor(path);
    menu = { path, x, y, items: [
      { kind: "header", label: path },
      {
        kind: "item", label: inherited ? `Ignored by ${ignoredBy}` : ignoredBy ? "Show folder again" : "Ignore folder",
        icon: ignoredBy ? Eye : EyeOff,
        disabled: inherited || folders.saving,
        run: () => void folders.setIgnored(path, !ignoredBy),
      },
      { kind: "sep" },
      { kind: "item", label: "Create a project for this folder", icon: FolderPlus, run: () => void createProject(path) },
      ...(branchActions.length ? [{ kind: "sep" as const }, ...branchActions] : []),
    ] };
  }

  // Wait for the catalog and saved scope before removing missing folder paths.
  $effect(() => {
    if (preview || catalog.preloading || catalog.scanning || session.restoring) return;
    folders.reconcileScope(collectFolderPaths(tree));
  });

  $effect(() => {
    if (preview && scroller) scroller.scrollTop = previewScroll;
  });

  // Drag-to-resize the panel. Dragging the inner edge past COLLAPSE_AT hides
  // the panel altogether (a peek arrow in +page.svelte brings it back).
  const MIN_W = 160;
  const MAX_W = 480;
  const COLLAPSE_AT = 120;

  const right = $derived((preview?.side ?? dock.folderTree) === "right");

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

  // On a narrow screen the tree floats over the grid, at this width (`--float-w` in the style).
  const FLOAT_PERCENT = 75;
  const FLOAT_MAX = 240;
  const ds = $derived(preview ? DOCK_IDLE : dock.state("folderTree"));
  const narrow = () => window.matchMedia(NARROW).matches;
  function thicknessFor(): number {
    if (!narrow()) return session.folderTreeWidth;
    return Math.min((FLOAT_PERCENT / 100) * (aside?.parentElement?.clientWidth ?? 0), FLOAT_MAX);
  }

  function startResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onResizeMove(e: PointerEvent) {
    if (!resizing || !aside) return;
    const rect = aside.getBoundingClientRect();
    const w = right ? rect.right - e.clientX : e.clientX - rect.left;
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
  {#if !preview}
    <DockGhost panel="folderTree">
      {#snippet ghost(side, size)}
        <FolderTree preview={{ side, size }} previewScroll={scrollTop} />
      {/snippet}
    </DockGhost>
  {/if}
  <aside
    class="tree dock-panel"
    class:preview={!!preview}
    class:right
    class:pending-collapse={pendingCollapse}
    class:docking={ds.docking}
    class:dock-moving={ds.moving}
    class:vacating={ds.vacating}
    bind:this={aside} style="--tree-w: {preview?.size ?? session.folderTreeWidth}px; --float-w: min({FLOAT_PERCENT}%, {FLOAT_MAX}px)">
    <div
      class="scroll"
      id={preview ? undefined : "folder-tree-scroll"}
      bind:this={scroller}
      bind:clientHeight={viewportH}
      onscroll={() => scroller && (scrollTop = scroller.scrollTop)}
    >
      <div class="inner" bind:clientHeight={contentH}>
        <div class="header">
          <DockGrip panel="folderTree" still={!!preview} floats={narrow} target={() => aside?.parentElement ?? null} {thicknessFor} orientation="horizontal">
            <span>Folders</span>
          </DockGrip>
        </div>
        <button class="node root" class:active={folders.allSelected} aria-pressed={folders.allSelected} class:menu-target={menu?.path === ""}
          use:folderContextMenu={openAllMenu} onclick={(e) => { showAll(); e.currentTarget.blur(); }}>
          <Images size={13} />
          <span class="name">All</span>
          <span class="count">{tree.count} {tree.count === 1 ? "file" : "files"}</span>
        </button>
        {#each children as child (child.path)}
          <FolderTreeNode node={child} depth={0} onmenu={openMenu} onselect={selectFolder} menuPath={menu?.path ?? null} />
        {/each}
      </div>
    </div>
    {#if !preview}
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
    <!-- Inner-edge resize handle (hidden on narrow screens where the tree floats). -->
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
    {/if}
    <!-- Hide tab on the panel's inner edge, pointing to the outer edge to collapse.
         Overlaid (absolute) so it costs no layout width; sits above the resize
         handle and overlay scrollbar (higher z-index) so it stays clickable. -->
    {#if preview}
      <span class="tree-hide">
        {#if right}<ChevronRight size={16} />{:else}<ChevronLeft size={16} />{/if}
      </span>
    {:else}
      <button
        class="tree-hide"
        title={shortcutHint("Hide folder tree", "ui.toggleFolderTree")}
        aria-label="Hide folder tree"
        onclick={() => (session.folderTreeVisible = false)}
      >
        {#if right}<ChevronRight size={16} />{:else}<ChevronLeft size={16} />{/if}
      </button>
    {/if}
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

  /* The tree is being dragged to another side. */
  .tree.docking {
    opacity: 0.5;
  }

  /* On a drag, and on the way back after a cancel. Never on a drop, so the
     landed panel and the content gap swap in one frame. */
  .tree.dock-moving {
    transition: width var(--dock-motion), opacity var(--dock-motion);
  }

  /* The preview is at another side: the tree closes its slot, so the grid takes
     the room. The node stays mounted, because the grip in it holds the pointer
     capture of the drag. */
  .tree.docking.vacating {
    width: 0;
    opacity: 0;
    overflow: hidden;
    border-width: 0;
  }

  .tree.docking.vacating .scroll {
    min-width: var(--tree-w);
  }

  /* A ghost fills the box that DockGhost gives it. */
  .tree.preview {
    position: relative;
    inset: auto;
    z-index: auto;
    width: 100%;
    height: 100%;
    box-shadow: none;
  }

  .tree.right {
    order: 1;
    border-right: none;
    border-left: 1px solid var(--border);
  }

  .scroll :global(button:focus-visible),
  .header :global(button:focus-visible) {
    outline-offset: -2px;
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

  /* Grabbable strip straddling the inner edge; widens the hit area without a
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

  .right .resize-handle {
    right: auto;
    left: -3px;
    /* Above the grid's overlay scrollbar (26), which ends at this edge. */
    z-index: 27;
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
      /* Above the grid's overlay scrollbar (26), which ends under this edge. */
      z-index: 27;
      width: var(--float-w);
      box-shadow: 4px 0 18px rgba(0, 0, 0, 0.55);
    }

    .tree.right {
      left: auto;
      right: 0;
      box-shadow: -4px 0 18px rgba(0, 0, 0, 0.55);
    }

    .resize-handle {
      display: none;
    }
  }

  /* Inner-edge hide tab (same 18x44 size, surface fill, accent chevron as
     .tree-peek). Anchored to the panel's inner edge and layered above the
     resize handle (z-index 25 left, 27 right) and overlay scrollbar (z-index 26)
     so its click target wins. */
  .tree-hide {
    position: absolute;
    top: 50%;
    right: 0;
    transform: translateY(-50%);
    z-index: 28;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 44px;
    padding: 0;
    border: 1px solid var(--border);
    border-right: none;
    border-radius: var(--r-md) 0 0 var(--r-md);
    background: var(--surface);
    color: var(--accent);
    cursor: pointer;
  }

  .tree-hide:hover {
    background: var(--hover);
  }

  .right .tree-hide {
    right: auto;
    left: 0;
    border-right: 1px solid var(--border);
    border-left: none;
    border-radius: 0 var(--r-md) var(--r-md) 0;
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
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  /* The whole header row is the grip, so the grip carries the padding. */
  .tree .header :global(.dock-grip.labelled) {
    padding: 8px 10px;
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
