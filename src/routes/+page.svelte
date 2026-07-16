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
  import ShortcutsOverlay from "$lib/components/ShortcutsOverlay.svelte";
  import PairSyncDialog from "$lib/components/PairSyncDialog.svelte";
  import TagEditor from "$lib/components/TagEditor.svelte";
  import CommitDialog from "$lib/components/CommitDialog.svelte";
  import MoveDialog from "$lib/components/MoveDialog.svelte";
  import ConfirmDialog from "$lib/components/ConfirmDialog.svelte";
  import SettingsDialog from "$lib/components/SettingsDialog.svelte";
  import PreviewModeIntro from "$lib/components/PreviewModeIntro.svelte";
  import ProjectGallery from "$lib/components/ProjectGallery.svelte";
  import AlertDialog from "$lib/components/AlertDialog.svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import type { PreviewMode } from "$lib/api";
  import { api } from "$lib/api";
  import Grid3x3 from "@lucide/svelte/icons/grid-3x3";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import Link from "@lucide/svelte/icons/link";
  import Unlink from "@lucide/svelte/icons/unlink";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Tag from "@lucide/svelte/icons/tag";
  import Type from "@lucide/svelte/icons/type";
  import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
  import ImageIcon from "@lucide/svelte/icons/image";
  import VideoIcon from "@lucide/svelte/icons/video";
  import FolderTreeIcon from "@lucide/svelte/icons/folder-tree";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import FolderGit2 from "@lucide/svelte/icons/folder-git-2";
  import { openUrl } from "@tauri-apps/plugin-opener";

  const REPO_URL = "https://github.com/la-lo-go/cullant";

  // Desktop opt-in for the touch action bar (always shown on touch devices).
  let touchBarVisible = $state(false);
  let showKeybindings = $state(false);
  let showSettings = $state(false);
  let showCloseConfirm = $state(false);
  // Set when the open project's folder/volume becomes unreachable while working.
  let folderLostMsg = $state("");

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

  // Returning to the grid (via Escape/back or the Grid button) can leave a
  // toolbar control focused — on mobile that shows a lingering focus ring and
  // the focused control can swallow keys. Drop DOM focus once we're in the grid.
  // Safe on desktop: grid navigation is driven by the global window keydown, so
  // nothing in the grid needs to hold focus.
  $effect(() => {
    if (view.mode === "grid") {
      (document.activeElement as HTMLElement | null)?.blur();
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
    // The read-only shortcuts cheat-sheet sits above everything else.
    if (view.shortcutsOpen) {
      view.shortcutsOpen = false;
      return;
    }
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

  // Custom window chrome is Windows-only: macOS/Linux keep native decorations
  // (restored in the Rust setup hook), and touch/mobile never gets a titlebar.
  // Detected via userAgent (consistent with the rest of the app) plus a
  // coarse-pointer check so Windows tablets in touch mode stay native.
  const showTitleBar =
    navigator.userAgent.includes("Windows") &&
    !navigator.userAgent.includes("Android") &&
    !window.matchMedia("(pointer: coarse)").matches;

  // The project name lives in the window title (OS taskbar/Alt-Tab, and the
  // custom TitleBar below) rather than the toolbar — kept out of the way
  // there, still one glance/hover away from anyone who needs the full path.
  $effect(() => {
    document.title = catalog.project ? `${catalog.project.displayName} — Cullant` : "Cullant";
  });

  // Whether the open project has any subfolders — used to hide the folder
  // tree toggle when there's nothing to scope by.
  const hasSubfolders = $derived(buildFolderTree(catalog.items).children.size > 0);

  // Whether the active tab has any RAW files — used to hide the Mirror/Separate
  // toggle when there are no RAWs to fan actions out to.
  const hasRaws = $derived(catalog.items.some((i) => i.kind === 0));

  // Watch the open project's storage while working: if its folder/volume goes
  // away (drive unplugged, folder moved/deleted) warn once, and clear the
  // warning if it comes back. Desktop is reliable; on Android this depends on
  // the id form (SAF tree URI vs a restored app-dir path).
  $effect(() => {
    const proj = catalog.project;
    if (!proj) {
      folderLostMsg = "";
      return;
    }
    let wasOk = true;
    const check = async () => {
      try {
        const info = await api.probeStorage(proj.rootPath);
        if (info.state === "ok") {
          wasOk = true;
        } else if (wasOk) {
          wasOk = false;
          folderLostMsg =
            info.state === "disconnected"
              ? "The drive or volume holding this project is no longer connected. Reconnect it to keep working, or close the project."
              : "This project's folder can no longer be found — it may have been moved or deleted. Restore it, or close the project.";
        }
      } catch {
        // Ignore transient IPC errors; the next tick retries.
      }
    };
    void check();
    const id = setInterval(() => void check(), 4000);
    return () => clearInterval(id);
  });

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
    // Reject Cullant's own sidecar dir up front (the backend guards it too, so
    // the CULLANT_OPEN_PROJECT auto-open hook is covered regardless).
    if (/(^|[\\/])\.cullant([\\/]|$)/i.test(path)) {
      catalog.error =
        "This is Cullant's own data folder (.cullant), not a photo or video folder. " +
        "Pick the folder that contains your photos or videos instead.";
      return;
    }
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

</script>

<svelte:window onkeydown={handleKeydown} />

<main class="app" class:fullscreen={view.fullscreen}>
  {#if showTitleBar}
    <TitleBar
      onOpenNew={() => void pickProject()}
      onOpenRecent={(path) => void openProject(path)}
      onCloseProject={() => (showCloseConfirm = true)}
    />
  {/if}
  {#if catalog.project}
    {#if !view.fullscreen}
    <header class="toolbar">
      <div class="toolbar-left">
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
        {#if catalog.scanning}
          <span class="status scanning">Scanning… {catalog.scanFound || ""}</span>
        {:else if catalog.previewProgress.total > 0}
          <span
            class="status scanning previews-status"
            title="Generating previews in the background — photos you open jump the queue"
          >
            Previews… {catalog.previewProgress.done} / {catalog.previewProgress.total}
          </span>
        {/if}
      </div>
      <div class="toolbar-center">
        <div class="segmented">
          <button class:active={view.mode === "grid"} title="Grid (G)" onclick={blurring(() => (view.mode = "grid"))}><Grid3x3 size={14} /></button>
          <button class:active={view.mode === "viewer"} title="Loupe (E)" onclick={blurring(() => { session.ensureFocus(); view.mode = "viewer"; })}><Search size={14} /></button>
          <button class:active={view.mode === "compare"} title="Compare (C)" onclick={blurring(() => { session.ensureFocus(); view.mode = "compare"; })}><Columns2 size={14} /></button>
        </div>
      </div>
      <div class="toolbar-right">
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
        {#if view.mode !== "grid"}
          <button
            class="touchbar-toggle"
            class:active={touchBarVisible}
            title="Show/hide the action bar"
            aria-label="Show/hide the action bar"
            onclick={blurring(() => (touchBarVisible = !touchBarVisible))}
          >
            <SlidersHorizontal size={14} />
          </button>
        {/if}
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
            title="Sort & filter"
            onclick={blurring(() => (session.filtersPanelOpen = !session.filtersPanelOpen))}
          >
            <ListFilter size={14} />
            <span>Sort &amp; Filter</span>
          </button>
          {#if session.filtersPanelOpen}
            <FiltersPanel />
          {/if}
        </div>
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
      </div>
    </header>
    {/if}

    {#if catalog.preloading}
      <div class="preload">
        {#if catalog.scanning}
          <p class="phase">Scanning…{catalog.scanFound ? ` ${catalog.scanFound} found` : ""}</p>
          <div class="pbar indeterminate" role="progressbar" aria-label="Scanning project folder">
            <div class="pbar-fill"></div>
          </div>
        {:else}
          {@const pdone = catalog.thumbProgress.done}
          {@const ptotal = catalog.thumbProgress.total}
          {@const ppct = ptotal > 0 ? Math.round((pdone / ptotal) * 100) : 0}
          <p class="phase">
            Generating thumbnails… <span class="count">{pdone} / {ptotal > 0 ? ptotal : "?"}</span>
          </p>
          {#if ptotal > 0}
            <div
              class="pbar"
              role="progressbar"
              aria-label="Generating thumbnails"
              aria-valuemin="0"
              aria-valuemax={ptotal}
              aria-valuenow={pdone}
            >
              <div class="pbar-fill" style="width:{ppct}%"></div>
            </div>
            <p class="pct">{ppct}%</p>
          {:else}
            <div class="pbar indeterminate" role="progressbar" aria-label="Generating thumbnails">
              <div class="pbar-fill"></div>
            </div>
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
      <TouchActionBar
        forceShow={touchBarVisible && view.mode !== "grid"}
        hidden={(view.mode === "grid" && session.selectedIds.size === 0) || view.fullscreen}
      />
    {/if}
  {:else}
    <div class="home" class:centered={recent.list.length === 0}>
      <div class="welcome">
        <svg class="logo" viewBox="0 0 1369 1218" xmlns="http://www.w3.org/2000/svg" aria-hidden="true">
          <g transform="matrix(1,0,0,1,-1069.617379,-135.415161)">
            <g transform="matrix(3.103723,0,0,3.103723,154.590885,-721.515314)">
              <path
                d="M735.586,519.5C735.586,519.5 739.473,568.534 712.678,582.843C680.798,599.868 663.519,553.11 634.642,577.247C611.326,596.735 629.032,625.109 626.151,643.124C619.985,681.67 555.152,677.363 569.967,621.314C571.184,616.713 584.535,562.904 535.024,552.746C518.381,549.332 479.567,550.468 480.73,596.91C481.01,608.097 487.504,638.968 460.464,643.631C448.06,645.77 432.423,635.11 434.118,613.897C435.531,596.208 442.333,576.751 407.462,568.666C377.23,561.657 353.197,588.486 328.527,589.882C325.237,590.069 298.431,591.586 295.307,565.522C292.837,544.908 300.402,535.202 302.029,533.113C306.097,527.894 391.343,432.74 394.697,430.789C409.535,422.16 419.042,435.709 427.892,444.083C432.306,448.259 432.329,448.152 436.673,452.323C472.319,486.545 472.587,486.553 475.559,487.197C484.075,489.039 482.846,484.581 509.213,452.274C528.631,428.482 527.26,427.49 546.592,403.57C563.401,382.77 562.188,381.933 579.285,361.329C587.143,351.86 592.61,338.702 606.33,342.985C611.957,344.741 611.874,347.567 646.265,390.682C660.202,408.155 678.899,432.673 681.703,436.351C708.771,471.847 731.085,495.961 734.185,506.594C736.027,512.914 735.586,519.5 735.586,519.5Z"
                style="fill:rgb(63,223,202);"
              />
              <g transform="matrix(0.813344,0,0,0.813344,-674.196775,-1509.93443)">
                <circle cx="1324.072" cy="2261.054" r="65.142" style="fill:rgb(63,223,202);" />
              </g>
            </g>
          </g>
        </svg>
        <h1>Cullant</h1>
        <p>Fast, keyboard-first photo culling</p>
        <button class="primary" onclick={pickProject}>Open project…</button>
      </div>
      <ProjectGallery onopen={(path) => void openProject(path)} />
      <footer class="home-footer">
        <button class="opensource" onclick={() => (showSettings = true)}>
          <SettingsIcon size={13} />
          Settings
        </button>
        <button class="ghlink" onclick={() => openUrl(REPO_URL)}>
          <FolderGit2 size={15} />
          <span class="ghlink-full"
            >Cullant is free &amp; open source. Say hi or contribute on GitHub!</span
          >
          <span class="ghlink-short">Free &amp; open source · GitHub</span>
        </button>
      </footer>
    </div>
  {/if}

  {#if catalog.error}
    <AlertDialog title="Couldn't open project" message={catalog.error} onclose={() => (catalog.error = "")} />
  {/if}

  {#if folderLostMsg}
    <AlertDialog title="Project folder unavailable" message={folderLostMsg} onclose={() => (folderLostMsg = "")} />
  {/if}

  {#if introPath !== null}
    <PreviewModeIntro onstart={(mode) => void confirmIntro(mode)} oncancel={cancelIntro} />
  {/if}

  {#if view.shortcutsOpen}
    <ShortcutsOverlay onclose={() => (view.shortcutsOpen = false)} />
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

  /* Kill the WebView's default tap-highlight flash (a blue overlay on every
     tapped element) app-wide. TouchActionBar already set this on its own
     buttons; every other button/toggle/icon (toolbar, dialogs, folder tree,
     grid cells) is just as tappable and was still flashing it. */
  :global(*) {
    -webkit-tap-highlight-color: transparent;
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
       .app derives from them (all 0 — the app is always fully inset). */
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
    /* Inset the whole app so nothing ever sits under the system bars or cutout,
       and zero out the --safe-* vars that edge-hugging children (touch bar,
       filmstrip…) consume. The TOP inset is deliberately left off here: the
       toolbar (or the home screen) pads itself by --inset-top instead, so the
       toolbar's surface colour bleeds up into the status-bar strip. */
    padding: 0 var(--inset-right) var(--inset-bottom) var(--inset-left);
    --safe-top: 0px;
    --safe-right: 0px;
    --safe-bottom: 0px;
    --safe-left: 0px;
  }

  /* Full-picture (fullscreen) hides the toolbar, which normally pads the top
     status-bar inset — so pad it here instead, keeping the image and its
     overlaid controls clear of the notch / status bar. Zero on desktop. */
  .app.fullscreen {
    padding-top: var(--inset-top);
  }

  .toolbar {
    display: flex;
    align-items: center;
    gap: 8px;
    /* The toolbar paints the top status-bar strip: its --surface-2 background
       bleeds up under the system status bar (the top inset is not applied on
       .app), so the strip reads as one continuous colour with the toolbar. */
    padding: calc(4px + var(--inset-top)) calc(10px + var(--safe-right)) 4px
      calc(10px + var(--safe-left));
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    flex: none;
  }

  /* Three-zone layout so the view-mode selector can sit truly centered: left
     and right zones share the remaining space equally, each wrapping its own
     buttons internally on narrow screens instead of the whole toolbar
     reflowing. The center zone never grows past its content. */
  .toolbar-left,
  .toolbar-right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1 1 0;
    min-width: 0;
    flex-wrap: wrap;
    row-gap: 4px;
  }

  .toolbar-right {
    justify-content: flex-end;
  }

  .toolbar-center {
    flex: 0 0 auto;
  }

  /* On touch devices the action bar is always visible, so its toggle is
     redundant — hide it there. */
  @media (pointer: coarse) {
    /* Mobile shows preview-generation progress only in the grid's bottom-right
       "Loading previews…" pill, not in the toolbar. */
    .toolbar .previews-status {
      display: none;
    }
    .toolbar .touchbar-toggle {
      display: none;
    }
  }

  /* Narrow screens: left + center share the first row (flex:1 still lets
     center sit after left rather than truly mid-viewport, but there's no
     third zone competing for that row's space); the right zone claims a full
     row of its own (flex-basis 100% forces the wrap) and wraps its own
     buttons across as many further rows as it needs. */
  @media (max-width: 720px) {
    .toolbar {
      flex-wrap: wrap;
      row-gap: 4px;
    }
    .toolbar-right {
      flex: 1 1 100%;
      justify-content: flex-start;
    }
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

  .preload .count {
    font-variant-numeric: tabular-nums;
    opacity: 0.95;
  }

  /* Custom progress bar: a track + an accent fill, replacing the native
     <progress> element (which ignored the theme and looked out of place). */
  .preload .pbar {
    position: relative;
    width: 320px;
    max-width: 80vw;
    height: 6px;
    border-radius: 999px;
    background: var(--surface-2);
    border: 1px solid var(--border);
    overflow: hidden;
  }

  .preload .pbar-fill {
    height: 100%;
    background: var(--accent);
    border-radius: inherit;
    transition: width 120ms linear;
  }

  /* Indeterminate (scanning / unknown total): a sliver sweeps the track. */
  .preload .pbar.indeterminate .pbar-fill {
    width: 40%;
    animation: preload-slide 1.1s ease-in-out infinite;
  }

  @keyframes preload-slide {
    0% {
      transform: translateX(-120%);
    }
    100% {
      transform: translateX(320%);
    }
  }

  .preload .pct {
    margin: 0;
    font-size: 12px;
    opacity: 0.6;
    font-variant-numeric: tabular-nums;
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
    /* No toolbar on the welcome screen, so the home pane itself pads the top
       inset (the toolbar does this everywhere else — see .toolbar). */
    padding: var(--inset-top) var(--safe-right) 0 var(--safe-left);
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

  /* Portrait mobile has no safe-area cutout to speak of, so the padding above
     resolves to ~0 — the grid otherwise ran cells edge to edge. A modest fixed
     margin (matching the toolbar's own 10px) reads far less cramped, and still
     grows past that for an actual cutout via max(). */
  @media (max-width: 720px) {
    .grid-area {
      padding-left: max(10px, var(--safe-left));
      padding-right: max(10px, var(--safe-right));
    }
  }

  .grid-area :global(.viewport) {
    flex: 1;
  }

  /* Peek tab shown at the left edge when the folder tree is collapsed. It
     floats over the grid (absolute) instead of sitting in the flex row, so on
     narrow screens it never steals a thumbnail column. Its hit area is tiny, so
     the grid underneath still scrolls everywhere else. */
  .tree-peek {
    position: absolute;
    top: 50%;
    left: 0;
    transform: translateY(-50%);
    z-index: 15;
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

  .welcome .logo {
    width: 76px;
    height: auto;
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

  /* Full label on desktop; a compact label on narrow screens so it never
     overflows or wraps awkwardly. */
  button.ghlink .ghlink-short {
    display: none;
  }

  @media (max-width: 600px) {
    button.ghlink {
      text-align: center;
      text-wrap: balance;
    }

    button.ghlink .ghlink-full {
      display: none;
    }

    button.ghlink .ghlink-short {
      display: inline;
    }
  }
</style>
