<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { session } from "$lib/stores/session.svelte";
  import { recent } from "$lib/stores/recent.svelte";
  import { tags } from "$lib/stores/tags.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { handleKeydown } from "$lib/keyboard/dispatcher.svelte";
  import VirtualGrid from "$lib/components/VirtualGrid.svelte";
  import FolderTree from "$lib/components/FolderTree.svelte";
  import Viewer from "$lib/components/Viewer.svelte";
  import CompareView from "$lib/components/CompareView.svelte";
  import FiltersPanel from "$lib/components/FiltersPanel.svelte";
  import SelectionBar from "$lib/components/SelectionBar.svelte";
  import TouchActionBar from "$lib/components/TouchActionBar.svelte";
  import { buildFolderTree } from "$lib/components/folderTree";
  import KeybindingsDialog from "$lib/components/KeybindingsDialog.svelte";
  import PairSyncDialog from "$lib/components/PairSyncDialog.svelte";
  import TagEditor from "$lib/components/TagEditor.svelte";
  import CommitDialog from "$lib/components/CommitDialog.svelte";
  import MoveDialog from "$lib/components/MoveDialog.svelte";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import ProjectGallery from "$lib/components/ProjectGallery.svelte";
  import { api, type SortKey } from "$lib/api";
  import Grid3x3 from "@lucide/svelte/icons/grid-3x3";
  import Search from "@lucide/svelte/icons/search";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Link from "@lucide/svelte/icons/link";
  import Unlink from "@lucide/svelte/icons/unlink";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Tag from "@lucide/svelte/icons/tag";
  import Type from "@lucide/svelte/icons/type";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import ImageIcon from "@lucide/svelte/icons/image";
  import VideoIcon from "@lucide/svelte/icons/video";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import Filter from "@lucide/svelte/icons/filter";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import { openUrl } from "@tauri-apps/plugin-opener";

  const REPO_URL = "https://github.com/la-lo-go/cullant";

  let showKeybindings = $state(false);
  let showCloseConfirm = $state(false);

  // Load the project's tag list + pending queue whenever a project opens.
  $effect(() => {
    if (catalog.project) {
      void tags.refresh();
      void session.refreshPending();
    }
  });

  // Whether the open project has any subfolders — used to disable the folder
  // tree toggle when there's nothing to scope by.
  const hasSubfolders = $derived(buildFolderTree(catalog.items).children.size > 0);

  async function pickProject() {
    // Android has no filesystem folder dialog; use the SAF tree picker, which
    // returns a content:// URI. Desktop uses the native directory dialog.
    const path = navigator.userAgent.includes("Android")
      ? await api.pickSafTree()
      : await open({ directory: true, title: "Open project folder" });
    if (path) await catalog.open(path);
  }

  function onSortChange(e: Event) {
    const el = e.currentTarget as HTMLSelectElement;
    catalog.setSort(el.value as SortKey);
    // Drop DOM focus, or ←/→ would change the sort instead of navigating.
    el.blur();
  }

  /**
   * Wrap a toolbar click handler so the control releases DOM focus afterwards.
   * Focused buttons/selects would otherwise swallow arrow-key navigation.
   */
  function blurring(fn: () => void): (e: Event) => void {
    return (e) => {
      fn();
      (e.currentTarget as HTMLElement).blur();
    };
  }

  /** Show only the last `maxSegments` path segments, with a leading ellipsis. */
  function truncatePath(path: string, maxSegments = 3): string {
    const segments = path.split(/[\\/]+/).filter(Boolean);
    if (segments.length <= maxSegments) return path;
    return "…" + "\\" + segments.slice(-maxSegments).join("\\");
  }
</script>

<svelte:window onkeydown={handleKeydown} />

<main class="app">
  {#if catalog.project}
    <header class="toolbar">
      <span class="title">Cullant</span>
      <span class="path" title={catalog.project.rootPath}>{truncatePath(catalog.project.rootPath)}</span>
      <div class="media-toggle">
        <button
          class="media-btn"
          class:active={catalog.media === "photos"}
          disabled={catalog.mediaCounts.photos === 0}
          title={catalog.mediaCounts.photos === 0 ? "No photos in this project" : "Show photos"}
          onclick={blurring(() => void catalog.setMedia("photos").then(() => session.clampFocus()))}
        >
          <ImageIcon size={14} />
          <span>Photos</span>
          <span class="count">{catalog.mediaCounts.photos}</span>
        </button>
        <button
          class="media-btn"
          class:active={catalog.media === "videos"}
          disabled={catalog.mediaCounts.videos === 0}
          title={catalog.mediaCounts.videos === 0 ? "No videos in this project" : "Show videos"}
          onclick={blurring(() => void catalog.setMedia("videos").then(() => session.clampFocus()))}
        >
          <VideoIcon size={14} />
          <span>Videos</span>
          <span class="count">{catalog.mediaCounts.videos}</span>
        </button>
      </div>
      <span class="spacer"></span>
      {#if catalog.scanning}
        <span class="status scanning">Scanning… {catalog.scanFound || ""}</span>
      {/if}
      <div class="segmented">
        <button class:active={view.mode === "grid"} title="Grid (G)" onclick={blurring(() => (view.mode = "grid"))}><Grid3x3 size={14} /></button>
        <button class:active={view.mode === "viewer"} title="Loupe (E)" onclick={blurring(() => (view.mode = "viewer"))}><Search size={14} /></button>
        <button class:active={view.mode === "compare"} title="Compare (C)" onclick={blurring(() => (view.mode = "compare"))}><Columns2 size={14} /></button>
      </div>
      <button
        class:active={session.mirrorMode}
        title="Mirror mode: RAW+JPEG pairs act as one photo (M)"
        onclick={blurring(() => session.setMirrorMode(!session.mirrorMode))}
      >
        {#if session.mirrorMode}<Link size={14} /><span>Mirror</span>{:else}<Unlink size={14} /><span>Separate</span>{/if}
      </button>
      <button
        class:active={session.showNames}
        title="Show/hide file names"
        onclick={blurring(() => session.toggleShowNames())}
      >
        <Type size={14} />
      </button>
      <button
        class:active={session.folderTreeVisible}
        disabled={!hasSubfolders}
        title={hasSubfolders ? "Show/hide folder tree (D)" : "No subfolders in this project"}
        onclick={blurring(() => (session.folderTreeVisible = !session.folderTreeVisible))}
      >
        <FolderTreeIcon size={14} />
      </button>
      <div class="filters-anchor">
        <button
          class:active={session.filtersPanelOpen}
          class:haswork={session.hasActiveFilters}
          title="Filters"
          onclick={blurring(() => (session.filtersPanelOpen = !session.filtersPanelOpen))}
        >
          <Filter size={14} />
          <span>Filters</span>
        </button>
        {#if session.filtersPanelOpen}
          <FiltersPanel />
        {/if}
      </div>
      <select value={catalog.sort} onchange={onSortChange}>
        <option value="capture">Capture time</option>
        <option value="name">Name</option>
      </select>
      <button
        class="commit"
        class:haswork={session.pendingCount > 0}
        title="Review & commit pending actions (Ctrl+Enter)"
        onclick={blurring(() => (session.commitDialogOpen = true))}
      >
        Commit{session.pendingCount > 0 ? ` (${session.pendingCount})` : ""}
      </button>
      <button title="Rescan project folder" onclick={blurring(() => void api.rescanProject())}><RefreshCw size={14} /></button>
      <button title="Task tags" onclick={blurring(() => (tags.editorOpen = true))}><Tag size={14} /></button>
      <button title="Keyboard shortcuts" onclick={blurring(() => (showKeybindings = true))}><Keyboard size={14} /></button>
      <button onclick={blurring(() => (showCloseConfirm = true))}>Close project</button>
    </header>

    {#if catalog.preloading}
      <div class="preload">
        {#if catalog.scanning}
          <p class="phase">Scanning… {catalog.scanFound || ""}</p>
          <progress></progress>
        {:else}
          <p class="phase">
            Generating thumbnails… {catalog.thumbProgress.done} / {catalog.thumbProgress.total > 0
              ? catalog.thumbProgress.total
              : "?"}
          </p>
          {#if catalog.thumbProgress.total > 0}
            <progress max={catalog.thumbProgress.total} value={catalog.thumbProgress.done}></progress>
          {:else}
            <progress></progress>
          {/if}
        {/if}
      </div>
    {:else}
      {#if session.selectedIds.size > 0 && view.mode === "grid"}
        <SelectionBar />
      {/if}

      {#if view.mode === "grid"}
        <div class="grid-area">
          {#if session.folderTreeVisible}
            <FolderTree />
          {/if}
          <VirtualGrid items={session.filtered} />
        </div>
      {:else if view.mode === "viewer"}
        <Viewer />
      {:else}
        <CompareView />
      {/if}
      <TouchActionBar />
    {/if}
  {:else}
    <div class="home" class:centered={recent.list.length === 0}>
      <div class="welcome">
        <h1>Cullant</h1>
        <p>Fast, keyboard-first photo culling</p>
        <button class="primary" onclick={pickProject}>Open project…</button>
        {#if catalog.error}
          <p class="error">{catalog.error}</p>
        {/if}
        <button class="opensource" onclick={() => openUrl(REPO_URL)}>
          <ExternalLink size={13} />
          Open source on GitHub
        </button>
      </div>
      <ProjectGallery onopen={(path) => void catalog.open(path)} />
    </div>
  {/if}

  {#if showKeybindings}
    <KeybindingsDialog onclose={() => (showKeybindings = false)} />
  {/if}

  {#if session.recoupleDialogFor !== null}
    <PairSyncDialog groupId={session.recoupleDialogFor} />
  {/if}

  {#if tags.editorOpen}
    <TagEditor onclose={() => (tags.editorOpen = false)} />
  {/if}

  {#if session.commitDialogOpen}
    <CommitDialog />
  {/if}

  {#if session.moveDialogOpen}
    <MoveDialog />
  {/if}

  {#if showCloseConfirm}
    <ConfirmDialog
      title="Close project?"
      message="This closes the current project and returns to the welcome screen. Nothing is committed or deleted."
      confirmLabel="Close project"
      onconfirm={() => {
        showCloseConfirm = false;
        void catalog.close();
      }}
      oncancel={() => (showCloseConfirm = false)}
    />
  {/if}
</main>

<style>
  :global(html, body) {
    margin: 0;
    height: 100%;
    overflow: hidden;
  }

  :global(:root) {
    font-family: Inter, "Segoe UI", Avenir, Helvetica, Arial, sans-serif;
    font-size: 14px;
    color: #e8e8e8;
    background-color: #1b1b1f;
    color-scheme: dark;
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 10px;
    background: #232329;
    border-bottom: 1px solid #333;
    flex: none;
  }

  /* Narrow screens: wrap the toolbar to a couple of rows and drop the least
     useful bits so every control stays reachable instead of overflowing. */
  @media (max-width: 720px) {
    .toolbar {
      flex-wrap: wrap;
      row-gap: 4px;
    }
    .toolbar .path,
    .toolbar .spacer {
      display: none;
    }
  }

  .title {
    font-weight: 700;
    font-size: 13px;
  }

  .path {
    opacity: 0.55;
    font-size: 12px;
    max-width: 30%;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .spacer {
    flex: 1;
  }

  .status {
    font-size: 12px;
    opacity: 0.75;
  }

  .status.scanning {
    color: #6bb2ff;
  }

  select,
  button {
    border-radius: 6px;
    border: 1px solid #3a3a42;
    padding: 4px 10px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: #2a2a30;
    cursor: pointer;
  }

  button:hover,
  select:hover {
    border-color: #6b6bff;
  }

  .toolbar button,
  .toolbar select {
    padding: 3px 8px;
    font-size: 12px;
  }

  .toolbar button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    min-height: 22px;
  }

  .preload {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    background: #1b1b1f;
  }

  .preload .phase {
    margin: 0;
    font-size: 13px;
    opacity: 0.8;
  }

  .preload progress {
    width: 320px;
  }


  button.active {
    border-color: #6b8bff;
  }

  .segmented {
    display: flex;
    gap: 2px;
  }

  button.commit.haswork {
    border-color: #ffb86b;
    color: #ffd9a8;
  }

  /* Anchors the Filters dropdown under its toolbar button. */
  .filters-anchor {
    position: relative;
    display: inline-flex;
  }

  /* Active-filters indicator on the Filters button. */
  .filters-anchor button.haswork {
    border-color: #6b8bff;
    color: #cfd9ff;
  }

  .segmented button {
    padding: 3px 7px;
  }

  .media-toggle {
    display: flex;
    gap: 3px;
    padding: 2px;
    border-radius: 8px;
    background: #1e1e23;
    border: 1px solid #333;
  }

  .media-btn {
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    padding: 3px 10px;
    font-weight: 600;
    font-size: 12px;
  }

  .media-btn:hover:not(:disabled) {
    border-color: #6b6bff;
  }

  .media-btn.active {
    background: #3a3a5c;
    border-color: #6b8bff;
    color: #fff;
  }

  .media-btn .count {
    font-weight: 400;
    opacity: 0.65;
    font-size: 11px;
  }

  .media-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .home {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }

  .home.centered {
    justify-content: center;
  }

  .grid-area {
    flex: 1;
    min-height: 0;
    display: flex;
    position: relative; /* anchors the folder-tree overlay on narrow screens */
  }

  .grid-area :global(.viewport) {
    flex: 1;
  }

  .welcome {
    flex: none;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
    padding: 48px 0 24px;
  }

  .welcome h1 {
    margin: 0;
  }

  .welcome p {
    margin: 0 0 1rem;
    opacity: 0.7;
  }

  button.primary {
    padding: 10px 22px;
    font-size: 15px;
  }

  .error {
    color: #ff6b6b;
  }

  button.opensource {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 0.75rem;
    border: none;
    background: none;
    padding: 2px 4px;
    font-size: 12px;
    color: #8fa6ff;
  }

  button.opensource:hover {
    color: #b0c0ff;
    text-decoration: underline;
  }
</style>
