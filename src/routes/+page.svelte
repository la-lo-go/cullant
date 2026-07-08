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
  import { api, type SortKey } from "$lib/api";

  let showKeybindings = $state(false);

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
    catalog.setSort((e.target as HTMLSelectElement).value as SortKey);
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
          onclick={() => catalog.setMedia("photos").then(() => session.clampFocus())}
        >
          Photos
        </button>
        <button
          class:active={catalog.media === "videos"}
          onclick={() => catalog.setMedia("videos").then(() => session.clampFocus())}
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
        <button class:active={view.mode === "grid"} title="Grid (G)" onclick={() => (view.mode = "grid")}>▦</button>
        <button class:active={view.mode === "viewer"} title="Loupe (E)" onclick={() => (view.mode = "viewer")}>🔍</button>
        <button class:active={view.mode === "compare"} title="Compare (C)" onclick={() => (view.mode = "compare")}>⿲</button>
      </div>
      <button
        class:active={session.mirrorMode}
        title="Mirror mode: RAW+JPEG pairs act as one photo (M)"
        onclick={() => (session.mirrorMode = !session.mirrorMode)}
      >
        {session.mirrorMode ? "🔗 Mirror" : "⛓ Separate"}
      </button>
      <select value={catalog.sort} onchange={onSortChange}>
        <option value="capture">Capture time</option>
        <option value="name">Name</option>
      </select>
      <button
        class="commit"
        class:haswork={session.pendingCount > 0}
        title="Review & commit pending actions (Ctrl+Enter)"
        onclick={() => (session.commitDialogOpen = true)}
      >
        Commit{session.pendingCount > 0 ? ` (${session.pendingCount})` : ""}
      </button>
      <button title="Rescan project folder" onclick={() => api.rescanProject()}>⟳</button>
      <button title="Task tags" onclick={() => (tags.editorOpen = true)}>🏷</button>
      <button title="Keyboard shortcuts" onclick={() => (showKeybindings = true)}>⌨</button>
      <button onclick={() => catalog.close()}>Close</button>
    </header>

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
