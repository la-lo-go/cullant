<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { session } from "$lib/stores/session.svelte";
  import { tags } from "$lib/stores/tags.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { handleKeydown } from "$lib/keyboard/dispatcher.svelte";
  import VirtualGrid from "$lib/components/VirtualGrid.svelte";
  import Viewer from "$lib/components/Viewer.svelte";
  import CompareView from "$lib/components/CompareView.svelte";
  import FilterBar from "$lib/components/FilterBar.svelte";
  import KeybindingsDialog from "$lib/components/KeybindingsDialog.svelte";
  import PairSyncDialog from "$lib/components/PairSyncDialog.svelte";
  import TagEditor from "$lib/components/TagEditor.svelte";
  import CommitDialog from "$lib/components/CommitDialog.svelte";
  import MoveDialog from "$lib/components/MoveDialog.svelte";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import { api, type SortKey } from "$lib/api";
  import Grid3x3 from "@lucide/svelte/icons/grid-3x3";
  import Search from "@lucide/svelte/icons/search";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Link from "@lucide/svelte/icons/link";
  import Unlink from "@lucide/svelte/icons/unlink";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Tag from "@lucide/svelte/icons/tag";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";

  let showKeybindings = $state(false);
  let showCloseConfirm = $state(false);

  // Load the project's tag list + pending queue whenever a project opens.
  $effect(() => {
    if (catalog.project) {
      void tags.refresh();
      void session.refreshPending();
    }
  });

  async function pickProject() {
    const path = await open({ directory: true, title: "Open project folder" });
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
</script>

<svelte:window onkeydown={handleKeydown} />

<main class="app">
  {#if catalog.project}
    <header class="toolbar">
      <span class="title">Cullant</span>
      <span class="path" title={catalog.project.rootPath}>{catalog.project.rootPath}</span>
      <div class="segmented">
        <button
          class:active={catalog.media === "photos"}
          onclick={blurring(() => void catalog.setMedia("photos").then(() => session.clampFocus()))}
        >
          Photos
        </button>
        <button
          class:active={catalog.media === "videos"}
          onclick={blurring(() => void catalog.setMedia("videos").then(() => session.clampFocus()))}
        >
          Videos
        </button>
      </div>
      <span class="spacer"></span>
      {#if catalog.scanning}
        <span class="status scanning">Scanning… {catalog.scanFound || ""}</span>
      {:else}
        <span class="status">{catalog.items.length} photos</span>
      {/if}
      <div class="segmented">
        <button class:active={view.mode === "grid"} title="Grid (G)" onclick={blurring(() => (view.mode = "grid"))}><Grid3x3 size={14} /></button>
        <button class:active={view.mode === "viewer"} title="Loupe (E)" onclick={blurring(() => (view.mode = "viewer"))}><Search size={14} /></button>
        <button class:active={view.mode === "compare"} title="Compare (C)" onclick={blurring(() => (view.mode = "compare"))}><Columns2 size={14} /></button>
      </div>
      <button
        class:active={session.mirrorMode}
        title="Mirror mode: RAW+JPEG pairs act as one photo (M)"
        onclick={blurring(() => (session.mirrorMode = !session.mirrorMode))}
      >
        {#if session.mirrorMode}<Link size={14} /><span>Mirror</span>{:else}<Unlink size={14} /><span>Separate</span>{/if}
      </button>
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
        <span class="spin"><LoaderCircle size={36} /></span>
        {#if catalog.scanning}
          <p class="phase">Scanning… {catalog.scanFound || ""}</p>
          <progress></progress>
        {:else}
          <p class="phase">
            Generating thumbnails… {catalog.thumbProgress.done} / {catalog.thumbProgress.total}
          </p>
          {#if catalog.thumbProgress.total > 0}
            <progress max={catalog.thumbProgress.total} value={catalog.thumbProgress.done}></progress>
          {:else}
            <progress></progress>
          {/if}
        {/if}
      </div>
    {:else}
      {#if session.filterBarVisible && view.mode === "grid"}
        <FilterBar />
      {/if}

      {#if view.mode === "grid"}
        <VirtualGrid items={session.filtered} />
      {:else if view.mode === "viewer"}
        <Viewer />
      {:else}
        <CompareView />
      {/if}
    {/if}
  {:else}
    <div class="welcome">
      <h1>Cullant</h1>
      <p>Fast, keyboard-first photo culling</p>
      <button class="primary" onclick={pickProject}>Open project…</button>
      {#if catalog.error}
        <p class="error">{catalog.error}</p>
      {/if}
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
    gap: 12px;
    padding: 8px 12px;
    background: #232329;
    border-bottom: 1px solid #333;
    flex: none;
  }

  .title {
    font-weight: 700;
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

  .toolbar button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    min-height: 26px;
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

  .spin {
    display: inline-flex;
    color: #6b8bff;
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
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

  .segmented button {
    padding: 4px 8px;
  }

  .welcome {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 0.5rem;
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
</style>
