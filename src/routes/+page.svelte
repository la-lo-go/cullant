<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { session } from "$lib/stores/session.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { handleKeydown } from "$lib/keyboard/dispatcher.svelte";
  import VirtualGrid from "$lib/components/VirtualGrid.svelte";
  import Viewer from "$lib/components/Viewer.svelte";
  import CompareView from "$lib/components/CompareView.svelte";
  import FilterBar from "$lib/components/FilterBar.svelte";
  import KeybindingsDialog from "$lib/components/KeybindingsDialog.svelte";
  import PairSyncDialog from "$lib/components/PairSyncDialog.svelte";
  import type { SortKey } from "$lib/api";

  let showKeybindings = $state(false);

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
