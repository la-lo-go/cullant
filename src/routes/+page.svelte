<script lang="ts">
  import { open } from "@tauri-apps/plugin-dialog";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { session } from "$lib/stores/session.svelte";
  import { recent } from "$lib/stores/recent.svelte";
  import { tags } from "$lib/stores/tags.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { settings } from "$lib/stores/settings.svelte";
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
  import SettingsDialog from "$lib/components/SettingsDialog.svelte";
  import PreviewModeIntro from "$lib/components/PreviewModeIntro.svelte";
  import ProjectGallery from "$lib/components/ProjectGallery.svelte";
  import type { PreviewMode } from "$lib/api";
  import { api } from "$lib/api";
  import Grid3x3 from "@lucide/svelte/icons/grid-3x3";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Link from "@lucide/svelte/icons/link";
  import Unlink from "@lucide/svelte/icons/unlink";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Tag from "@lucide/svelte/icons/tag";
  import Type from "@lucide/svelte/icons/type";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import ImageIcon from "@lucide/svelte/icons/image";
  import VideoIcon from "@lucide/svelte/icons/video";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import Filter from "@lucide/svelte/icons/filter";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import FolderGit2 from "@lucide/svelte/icons/folder-git-2";
  import { openUrl } from "@tauri-apps/plugin-opener";

  const REPO_URL = "https://github.com/la-lo-go/cullant";

  // Desktop opt-in for the touch action bar (always shown on touch devices).
  let touchBarVisible = $state(false);
  let showKeybindings = $state(false);
  let showSettings = $state(false);
  let showCloseConfirm = $state(false);

  // First-run onboarding: the very first interactive open shows a one-time
  // welcome dialog to pick the preview loading mode. `introPath` stashes the
  // project the user asked to open while the dialog is up.
  let introPath = $state<string | null>(null);

  // Load the project's tag list + pending queue whenever a project opens.
  $effect(() => {
    if (catalog.project) {
      void tags.refresh();
      void session.refreshPending();
    }
  });

  /**
   * Unified "back" action shared by desktop and Android. Hierarchy:
   *   1. If any modal/overlay is open, close the top-most one (and stop).
   *   2. Else if in loupe/compare, return to the grid.
   *   3. Else (grid, nothing open) treat it as "close the project" and open the
   *      close-project confirmation. Backing out of THAT dialog hits rule 1.
   */
  function goBack() {
    // Top-most first, matching the visual stacking order of the dialogs below.
    if (showKeybindings) {
      showKeybindings = false;
      return;
    }
    if (showSettings) {
      showSettings = false;
      return;
    }
    if (session.recoupleDialogFor !== null) {
      session.recoupleDialogFor = null;
      return;
    }
    if (tags.editorOpen) {
      tags.editorOpen = false;
      return;
    }
    if (session.commitDialogOpen) {
      session.commitDialogOpen = false;
      return;
    }
    if (session.moveDialogOpen) {
      session.moveDialogOpen = false;
      return;
    }
    if (showCloseConfirm) {
      showCloseConfirm = false;
      return;
    }
    if (session.filtersPanelOpen) {
      session.filtersPanelOpen = false;
      return;
    }
    if (view.mode !== "grid") {
      view.mode = "grid";
      return;
    }
    // At the grid root: offer to close the project. On the welcome screen
    // (no project open) there's nothing to back out of, so do nothing.
    if (catalog.project) showCloseConfirm = true;
  }

  // History-API "trap" so the OS/browser back gesture never navigates away or
  // exits the app. We seed a history entry on mount, and every popstate (Android
  // hardware/gesture back, desktop mouse back button, Alt+Left) runs goBack()
  // then re-seeds another entry so subsequent backs stay captured. goBack()
  // always does something (down to showing the close-confirm), so we always
  // re-seed and never fall off our own history. Programmatic pushState does not
  // itself fire popstate, so there is no feedback loop.
  $effect(() => {
    history.pushState(null, "", location.href);
    const onPopState = () => {
      goBack();
      history.pushState(null, "", location.href);
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  });

  // Whether the open project has any subfolders — used to hide the folder
  // tree toggle when there's nothing to scope by.
  const hasSubfolders = $derived(buildFolderTree(catalog.items).children.size > 0);

  // Whether the active tab has any RAW files — used to hide the Mirror/Separate
  // toggle when there are no RAWs to fan actions out to.
  const hasRaws = $derived(catalog.items.some((i) => i.kind === 0));

  async function pickProject() {
    // Android has no filesystem folder dialog; use the SAF tree picker, which
    // returns a content:// URI. Desktop uses the native directory dialog.
    const path = navigator.userAgent.includes("Android")
      ? await api.pickSafTree()
      : await open({ directory: true, title: "Open project folder" });
    if (path) await openProject(path);
  }

  /**
   * Interactive open gate. On the very first open ever, stash the target path
   * and show the preview-mode intro instead of opening immediately; the dialog
   * confirms the choice and then opens. Afterwards, open directly.
   */
  async function openProject(path: string) {
    if (!settings.onboardedPreview) {
      introPath = path;
      return;
    }
    await catalog.open(path);
  }

  /** Intro confirmed: persist the chosen mode, mark onboarded, then open. */
  async function confirmIntro(mode: PreviewMode) {
    const path = introPath;
    introPath = null;
    if (path === null) return;
    settings.setPreviewMode(mode);
    settings.setOnboardedPreview(true);
    await catalog.open(path);
  }

  /**
   * Intro cancelled/Escaped: abort the open and leave `onboardedPreview` false
   * so the dialog reappears on the next attempt (least-surprising: the user
   * never gets a project loaded with a mode they didn't confirm).
   */
  function cancelIntro() {
    introPath = null;
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

<main class="app" class:edge={settings.edgeToEdge}>
  {#if catalog.project}
    <header class="toolbar">
      <span class="path" title={catalog.project.rootPath}>
        {catalog.project.rootPath.startsWith("content://")
          ? catalog.project.displayName
          : truncatePath(catalog.project.rootPath)}
      </span>
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
      {:else if catalog.previewProgress.total > 0}
        <span
          class="status scanning"
          title="Generating previews in the background — photos you open jump the queue"
        >
          Previews… {catalog.previewProgress.done} / {catalog.previewProgress.total}
        </span>
      {/if}
      <div class="segmented">
        <button class:active={view.mode === "grid"} title="Grid (G)" onclick={blurring(() => (view.mode = "grid"))}><Grid3x3 size={14} /></button>
        <button class:active={view.mode === "viewer"} title="Loupe (E)" onclick={blurring(() => (view.mode = "viewer"))}><Search size={14} /></button>
        <button class:active={view.mode === "compare"} title="Compare (C)" onclick={blurring(() => (view.mode = "compare"))}><Columns2 size={14} /></button>
      </div>
      {#if hasRaws}
        <button
          class:active={session.mirrorMode}
          title={session.mirrorMode
            ? "Mirror mode: RAW+JPEG pairs act as one photo — click to separate (M)"
            : "Separate mode: RAW and JPEG act independently — click to mirror (M)"}
          onclick={blurring(() => session.setMirrorMode(!session.mirrorMode))}
        >
          {#if session.mirrorMode}<Link size={14} /><span>Mirror</span>{:else}<Unlink size={14} /><span>Separate</span>{/if}
        </button>
      {/if}
      {#if view.mode === "grid"}
        <button
          class:active={session.showNames}
          title="Show/hide file names"
          onclick={blurring(() => session.toggleShowNames())}
        >
          <Type size={14} />
        </button>
      {/if}
      <button
        class="touchbar-toggle"
        class:active={touchBarVisible}
        title="Show/hide the action bar"
        aria-label="Show/hide the action bar"
        onclick={blurring(() => (touchBarVisible = !touchBarVisible))}
      >
        <PanelBottom size={14} />
      </button>
      {#if hasSubfolders && view.mode === "grid"}
        <button
          class:active={session.folderTreeVisible}
          title="Show/hide folder tree (D)"
          onclick={blurring(() => (session.folderTreeVisible = !session.folderTreeVisible))}
        >
          <FolderTreeIcon size={14} />
        </button>
      {/if}
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
      {#if view.mode === "grid"}
        <div class="segmented sortseg">
          <button
            class:active={catalog.sort === "capture"}
            title="Sort by capture time (click again to reverse)"
            onclick={blurring(() => void catalog.setSort("capture"))}
          >
            <span>Capture</span>
            {#if catalog.sort === "capture"}
              {#if catalog.sortDesc}<ArrowDown size={13} />{:else}<ArrowUp size={13} />{/if}
            {/if}
          </button>
          <button
            class:active={catalog.sort === "name"}
            title="Sort by name (click again to reverse)"
            onclick={blurring(() => void catalog.setSort("name"))}
          >
            <span>Name</span>
            {#if catalog.sort === "name"}
              {#if catalog.sortDesc}<ArrowDown size={13} />{:else}<ArrowUp size={13} />{/if}
            {/if}
          </button>
        </div>
      {/if}
      <button
        class="commit"
        class:haswork={session.pendingCount > 0}
        title="Review & commit pending actions (Ctrl+Enter)"
        onclick={blurring(() => (session.commitDialogOpen = true))}
      >
        Commit{session.pendingCount > 0 ? ` (${session.pendingCount})` : ""}
      </button>
      <button title="Rescan project folder" onclick={blurring(() => void api.rescanProject(settings.previewMode))}><RefreshCw size={14} /></button>
      <button title="Task tags" onclick={blurring(() => (tags.editorOpen = true))}><Tag size={14} /></button>
      <button title="Settings" onclick={blurring(() => (showSettings = true))}><SettingsIcon size={14} /></button>
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
          {:else if hasSubfolders}
            <button
              class="tree-peek"
              title="Show folder tree (D)"
              aria-label="Show folder tree"
              onclick={blurring(() => (session.folderTreeVisible = true))}
            >
              <ChevronRight size={16} />
            </button>
          {/if}
          <VirtualGrid items={session.filtered} />
        </div>
      {:else if view.mode === "viewer"}
        <Viewer />
      {:else}
        <CompareView />
      {/if}
      <TouchActionBar forceShow={touchBarVisible} />
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
      </div>
      <ProjectGallery onopen={(path) => void openProject(path)} />
      <footer class="home-footer">
        <button class="opensource" onclick={() => (showSettings = true)}>
          <SettingsIcon size={13} />
          Settings
        </button>
        <button class="ghlink" onclick={() => openUrl(REPO_URL)}>
          <FolderGit2 size={15} />
          <span>Cullant is free &amp; open source. Say hi or contribute on GitHub!</span>
        </button>
      </footer>
    </div>
  {/if}

  {#if introPath !== null}
    <PreviewModeIntro onstart={(mode) => void confirmIntro(mode)} oncancel={cancelIntro} />
  {/if}

  {#if showKeybindings}
    <KeybindingsDialog onclose={() => (showKeybindings = false)} />
  {/if}

  {#if showSettings}
    <SettingsDialog
      onclose={() => (showSettings = false)}
      onshowkeybindings={() => {
        showSettings = false;
        showKeybindings = true;
      }}
    />
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

  /* The single source of truth for the app's theme. Every component references
     these via var(--…); nothing hardcodes a background or accent hex. To
     retheme, change only the values here.
       - surfaces: a warm dark-teal ramp anchored on --bg (the grid/menu base),
         each step a touch lighter for panels → toolbar/dialogs → hover →
         controls → borders, so the depth hierarchy is preserved.
       - accent: a pale mint used for outlines/borders/focus/links/light text;
         --accent-fill is its DARK counterpart for filled active states (white
         text stays legible on it), and --accent-rgb feeds translucent tints. */
  :global(:root) {
    --bg-stage: #191d1e;
    --bg: #222728;
    --surface: #272d2e;
    --surface-2: #2c3334;
    --hover: #313839;
    --control: #384040;
    --border: #3d4544;
    --border-strong: #495251;

    --accent: #3fdfca;
    --accent-rgb: 63, 223, 202;
    --accent-fill: #227268;

    /* Raw safe-area insets (status bar, navigation bar, display cutout —
       Android/iOS). They follow the device orientation automatically, e.g. a
       landscape rotation moves the cutout inset to the left or right side.
       Always 0 on desktop. Fixed-position overlays (dialog backdrops) consume
       these directly; everything in normal flow consumes the --safe-* set that
       .app derives from them based on the edge-to-edge setting. */
    --inset-top: env(safe-area-inset-top, 0px);
    --inset-right: env(safe-area-inset-right, 0px);
    --inset-bottom: env(safe-area-inset-bottom, 0px);
    --inset-left: env(safe-area-inset-left, 0px);

    font-family: Inter, "Segoe UI", Avenir, Helvetica, Arial, sans-serif;
    font-size: 14px;
    color: #e8e8e8;
    background-color: var(--bg);
    color-scheme: dark;
  }

  /* App-styled scrollbars: a thin thumb in the app accent (mint) hugging
     the content edge, a fully transparent track so no bar ever reads as its
     own column. The 2px transparent border (clipped to content-box) insets the
     thumb so the visible bar is only ~4px. */
  :global(::-webkit-scrollbar) {
    width: 8px;
    height: 8px;
  }

  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }

  :global(::-webkit-scrollbar-thumb) {
    background-color: rgba(var(--accent-rgb), 0.5);
    border-radius: 8px;
    border: 2px solid transparent;
    background-clip: content-box;
  }

  :global(::-webkit-scrollbar-thumb:hover) {
    background-color: rgba(var(--accent-rgb), 0.85);
  }

  :global(::-webkit-scrollbar-corner) {
    background: transparent;
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
    height: 100dvh; /* track the real viewport across Android rotations */
    box-sizing: border-box;
    /* Edge-to-edge OFF (default): inset the whole app so nothing ever sits
       under the system bars or cutout, and zero out the --safe-* vars that
       edge-hugging children (toolbar, touch bar, filmstrip…) consume. */
    padding: var(--inset-top) var(--inset-right) var(--inset-bottom) var(--inset-left);
    --safe-top: 0px;
    --safe-right: 0px;
    --safe-bottom: 0px;
    --safe-left: 0px;
  }

  /* Edge-to-edge ON: backgrounds bleed under the system bars; each control
     that touches a screen edge pads itself by --safe-* to stay reachable. */
  .app.edge {
    padding: 0;
    --safe-top: var(--inset-top);
    --safe-right: var(--inset-right);
    --safe-bottom: var(--inset-bottom);
    --safe-left: var(--inset-left);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: calc(4px + var(--safe-top)) calc(10px + var(--safe-right)) 4px
      calc(10px + var(--safe-left));
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  /* On touch devices the action bar is always visible, so its toggle is
     redundant — hide it there. */
  @media (pointer: coarse) {
    .toolbar .touchbar-toggle {
      display: none;
    }
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
    color: var(--accent);
  }

  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 4px 10px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }

  .toolbar button {
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
    background: var(--bg);
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
    background: var(--accent-fill);
    border-color: transparent;
    color: #fff;
  }

  /* Same pill treatment as the Photos/Videos toggle. */
  .segmented {
    display: flex;
    gap: 3px;
    padding: 2px;
    border-radius: 8px;
    background: var(--surface);
    border: 1px solid var(--border);
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
    border-color: var(--accent);
    color: var(--accent);
  }

  .segmented button {
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    padding: 4px 9px;
    transition: background-color 0.15s ease, color 0.15s ease;
  }

  .segmented button:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .segmented button.active {
    background: var(--accent-fill);
    border-color: transparent;
    color: #fff;
    animation: pill-pop 0.15s ease;
  }

  /* Quick, subtle scale pop when a pill becomes active. Transform-only so it
     never shifts layout. */
  @keyframes pill-pop {
    0% {
      transform: scale(0.9);
    }
    100% {
      transform: scale(1);
    }
  }

  /* Sort buttons carry a label plus a direction arrow when active. */
  .sortseg button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 12px;
    font-weight: 600;
  }

  .media-toggle {
    display: flex;
    gap: 3px;
    padding: 2px;
    border-radius: 8px;
    background: var(--surface);
    border: 1px solid var(--border);
  }

  .media-btn {
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    padding: 3px 10px;
    font-weight: 600;
    font-size: 12px;
    transition: background-color 0.15s ease, color 0.15s ease;
  }

  .media-btn:hover:not(:disabled) {
    border-color: var(--accent);
  }

  .media-btn.active {
    background: var(--accent-fill);
    border-color: transparent;
    color: #fff;
    animation: pill-pop 0.15s ease;
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
    padding: var(--safe-top) var(--safe-right) 0 var(--safe-left);
  }

  .home.centered {
    justify-content: center;
  }

  .grid-area {
    flex: 1;
    min-height: 0;
    display: flex;
    position: relative; /* anchors the folder-tree overlay on narrow screens */
    /* Keep thumbnails out from under a landscape navigation bar / cutout. */
    padding-left: var(--safe-left);
    padding-right: var(--safe-right);
  }

  .grid-area :global(.viewport) {
    flex: 1;
  }

  /* Peek tab shown at the left edge when the folder tree is collapsed. */
  .tree-peek {
    flex: none;
    align-self: center;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 44px;
    padding: 0;
    border: 1px solid var(--border);
    border-left: none;
    border-radius: 0 6px 6px 0;
    background: var(--surface);
    color: var(--accent);
    cursor: pointer;
  }

  .tree-peek:hover {
    background: var(--hover);
    border-color: var(--accent);
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
    border: none;
    background: none;
    padding: 2px 4px;
    font-size: 12px;
    color: var(--accent);
  }

  button.opensource:hover {
    color: var(--accent);
    text-decoration: underline;
  }

  .home-footer {
    flex: none;
    margin-top: auto;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 6px;
    padding: 16px 0 calc(16px + var(--safe-bottom));
  }

  button.ghlink {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border: none;
    background: none;
    padding: 6px 10px;
    font-size: 13px;
    color: #8a8a93;
  }

  button.ghlink:hover {
    color: var(--accent);
  }
</style>
