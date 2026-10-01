<script lang="ts">
  import homeLogoUrl from "../../logo/logo-w.svg?url";
  import { untrack } from "svelte";
  import { open } from "@tauri-apps/plugin-dialog";
  import { addPluginListener, type PluginListener } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { IS_ANDROID, IS_MOBILE, IS_TOUCH, IS_WINDOWS } from "$lib/platform";
  import { catalog } from "$lib/stores/catalog.svelte";
  import { session, flushSessionSave } from "$lib/stores/session.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { recent } from "$lib/stores/recent.svelte";
  import { tags } from "$lib/stores/tags.svelte";
  import { view } from "$lib/stores/view.svelte";
  import { handleKeydown } from "$lib/keyboard/dispatcher.svelte";
  import { shortcutHint } from "$lib/keyboard/hints";
  import { formatColorLabel } from "$lib/colorLabels";
  import { folders } from "$lib/stores/folders.svelte";
  import VirtualGrid from "$lib/components/VirtualGrid.svelte";
  import FolderTree from "$lib/components/FolderTree.svelte";
  import Viewer from "$lib/components/Viewer.svelte";
  import CompareView from "$lib/components/CompareView.svelte";
  import SurveyView from "$lib/components/SurveyView.svelte";
  import FiltersPanel from "$lib/components/FiltersPanel.svelte";
  import GridViewPanel from "$lib/components/GridViewPanel.svelte";
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
  import SupportDialog from "$lib/components/SupportDialog.svelte";
  import InfoOverlay from "$lib/components/InfoOverlay.svelte";
  import ProjectGallery from "$lib/components/ProjectGallery.svelte";
  import SearchOverlay from "$lib/components/SearchOverlay.svelte";
  import AlertDialog from "$lib/components/AlertDialog.svelte";
  import TitleBar from "$lib/components/TitleBar.svelte";
  import { api } from "$lib/api";
  import Grid3x3 from "@lucide/svelte/icons/grid-3x3";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Search from "@lucide/svelte/icons/search";
  import Eye from "@lucide/svelte/icons/eye";
  import Columns2 from "@lucide/svelte/icons/columns-2";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import ImageIcon from "@lucide/svelte/icons/image";
  import VideoIcon from "@lucide/svelte/icons/video";
  import ListFilter from "@lucide/svelte/icons/list-filter";
  import LayoutGrid from "@lucide/svelte/icons/layout-grid";
  import ArrowUp from "@lucide/svelte/icons/arrow-up";
  import ArrowDown from "@lucide/svelte/icons/arrow-down";
  import SettingsIcon from "@lucide/svelte/icons/settings";
  import CheckCheck from "@lucide/svelte/icons/check-check";
  import Heart from "@lucide/svelte/icons/heart";

  // Compare/loupe action bar: default visible, and remember the user's last
  // toggle across sessions. (It only ever renders in loupe/compare — see the
  // `view.mode !== "grid"` gate on <TouchActionBar>.)
  const TOUCHBAR_KEY = "cullant.touchBarVisible";
  function loadTouchBarVisible(): boolean {
    try {
      const raw = localStorage.getItem(TOUCHBAR_KEY);
      return raw === null ? true : JSON.parse(raw) === true;
    } catch {
      return true;
    }
  }
  let touchBarVisible = $state(loadTouchBarVisible());
  // Dev-only: publish an ingest event trace on window for external UI driving.
  // Dynamic import so the module never reaches a production bundle.
  if (import.meta.env.DEV) {
    void import("$lib/dev/trace").then((m) => m.installDevTrace());
  }
  $effect(() => {
    try {
      localStorage.setItem(TOUCHBAR_KEY, JSON.stringify(touchBarVisible));
    } catch {
      // persistence is best-effort
    }
  });
  // Measured height of the bottom action bar (0 while it is hidden). Published
  // as --bottom-bar-h so overlays inside the grid — the background-status pill —
  // can sit above it instead of being buried by it in selection mode.
  let bottomBarH = $state(0);
  const flagNames = { all: "All", pick: "Picked", reject: "Rejected", unflagged: "Unflagged", anyflag: "Flagged", notrejected: "Not rejected" };
  const activeFilterSummary = $derived([
    session.nameFilter.trim() && `Filename: ${session.nameFilter}`,
  const dateNames: Record<string, string> = { today: "Today", last7days: "Last 7 days" };
    session.flagFilter !== "all" && `Flag: ${flagNames[session.flagFilter]}`,
    session.minRating > 0 && `Rating: ${session.minRating}+ stars`,
    session.labelFilter && `Label: ${formatColorLabel(session.labelFilter)}`,
    session.tagFilter !== null && `Tag: ${tags.all.find((tag) => tag.id === session.tagFilter)?.name ?? session.tagFilter}`,
    catalog.media === "photos" && session.typeFilter !== "all" && `Type: ${session.typeFilter.toUpperCase()}`,
    session.extFilter && `Extension: ${session.extFilter}`,
    session.orientationFilter !== "all" && `Orientation: ${session.orientationFilter}`,
    session.dateFilter && `Date: ${dateNames[session.dateFilter] ?? session.dateFilter}`,
    session.hasBursts && session.burstFilter !== "all" && `Burst: ${session.burstFilter}`,
    session.burstKeyFilter && "Burst: selected burst",
    ...Object.entries(folders.scope).map(([path, included]) => `${included ? "Folder" : "Excluded folder"}: ${path || "project root"}`),
    ...[ ["Camera", session.cameraFilter], ["Lens", session.lensFilter], ["ISO", session.isoFilter], ["Aperture", session.apertureFilter], ["Focal length", session.focalFilter], ["Shutter", session.shutterFilter] ]
      .filter(([, value]) => catalog.media === "photos" && value !== null)
      .map(([label, value]) => `${label}: ${value}`),
  ].filter(Boolean).join("; "));
  let showKeybindings = $state(false);
  let showSupport = $state(false);
  let showSettings = $state(false);
  let settingsQuery = $state("");
  /** Settings opens three dialogs of its own, and closes itself to do it.
   *  Backing out of one of those should land back in Settings rather than in the
   *  grid, so it remembers where the user came from. */
  let returnToSettings = $state(false);

  /** Close a dialog Settings opened, going back to Settings if that is where it
   *  was opened from. */
  function leaveSubDialog(close: () => void) {
    close();
    if (returnToSettings) {
      returnToSettings = false;
      showSettings = true;
    }
  }

  /** Open a dialog from Settings, remembering to come back to it. */
  function fromSettings(open: () => void) {
    showSettings = false;
    returnToSettings = true;
    open();
  }
  let showCloseConfirm = $state(false);
  let showReimportConfirm = $state(false);
  let folderLostMsg = $state("");
  // Mirrors the storage watcher's last verdict, so the auto-rescan below can skip
  // ticks while a drive is disconnected instead of probing a second time.
  let storageOk = $state(true);

  // Keep the backend's video-thumbnail preference in sync with the setting.
  // Runs once on mount (pushing the persisted value) and again on every toggle,
  // so the next ingest pass honors it without needing a reopen.
  $effect(() => {
    void api.setGenerateVideoThumbs(settings.generateVideoThumbs);
  });

  // Same for the preview size. Pushing it is idempotent and never deletes
  // anything, so running on mount (and on every change) is safe — Settings owns
  // the destructive half, after the user has confirmed it.
  $effect(() => {
    void api.setPreviewQuality(settings.previewQuality);
  });

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
   *   3. Else if items are selected in the grid, clear the selection.
   *   4. Else (grid, nothing open, nothing selected) treat it as "close the
   *      project" and open the close-project confirmation. Backing out of THAT
   *      dialog hits rule 1.
   */
  function goBack() {
    // Top-most first, matching the visual stacking order of the dialogs below.
    // The read-only shortcuts cheat-sheet sits above everything else.
    if (view.shortcutsOpen) {
      view.shortcutsOpen = false;
      return;
    }
    // An explanation floats above everything that can raise one.
    if (view.infoTip) {
      view.infoTip = null;
      return;
    }
    if (showKeybindings) {
      leaveSubDialog(() => (showKeybindings = false));
      return;
    }
    if (showSupport) {
      leaveSubDialog(() => (showSupport = false));
      return;
    }
    if (view.settingsPanel) {
      view.settingsPanel = null;
      return;
    }
    if (showSettings) {
      showSettings = false;
      settingsQuery = "";
      return;
    }
    if (session.recoupleDialogFor !== null) {
      session.recoupleDialogFor = null;
      return;
    }
    if (tags.editorOpen) {
      leaveSubDialog(() => (tags.editorOpen = false));
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
    if (showReimportConfirm) {
      showReimportConfirm = false;
      return true;
    }
    if (showCloseConfirm) {
      showCloseConfirm = false;
      return;
    }
    if (session.filtersPanelOpen) {
      session.filtersPanelOpen = false;
      return;
    }
    if (session.viewPanelOpen) {
      session.viewPanelOpen = false;
      return;
    }
    if (view.mode !== "grid") {
      view.mode = "grid";
      return;
    }
    // A selection is itself a "state" to back out of: clear it before the app
    // treats back as "leave the project" (mirrors Esc via the keyboard
    // dispatcher's view.back).
    if (session.selectedIds.size > 0) {
      session.clearSelection();
      return;
    }
    // At the grid root: offer to close the project. On the welcome screen
    // (no project open) there's nothing to back out of, so do nothing.
    if (catalog.project) showCloseConfirm = true;
  }

  // Back belongs to goBack() on every platform, but it reaches us by three
  // routes, all wired here.
  //
  // Desktop only ever sees it as a history navigation (mouse button 4,
  // Alt+Left), so we seed a history entry and let popstate stand in for it.
  //
  // Android hands it to native code first, and Tauri's built-in handler walks
  // the *WebView's* history, finishing the activity as soon as that runs out.
  // That stack is not ours to rely on — a backgrounded app can come back with a
  // recreated activity and a fresh WebView, and then back quit the app mid-cull
  // instead of running the hierarchy above. Registering a `back-button` plugin
  // listener takes the press off that path: the native handler hands it
  // straight to us and never consults history. `__cullantBack` is the second
  // native route, for the case where Tauri's handler is gone entirely — see the
  // callback in `src-tauri/gen/android/.../MainActivity.kt`.
  $effect(() => {
    const win = window as unknown as Record<string, unknown>;
    win.__cullantBack = goBack;

    const seed = () => history.pushState(null, "", location.href);
    seed();
    const onPopState = () => {
      // goBack() always consumes the press (down to showing the close-confirm),
      // so re-arm unconditionally and we never fall off our own history.
      // Programmatic pushState does not fire popstate, so there is no loop.
      goBack();
      seed();
    };
    // A resumed WebView can come back without the seeded entry. Re-arming costs
    // one duplicate entry per resume and the engine caps the stack for us.
    const onVisibility = () => {
      if (document.visibilityState === "visible") seed();
    };
    window.addEventListener("popstate", onPopState);
    document.addEventListener("visibilitychange", onVisibility);

    let listener: PluginListener | null = null;
    let disposed = false;
    if (IS_ANDROID) {
      void addPluginListener("app", "back-button", () => goBack())
        .then((l) => {
          if (disposed) void l.unregister();
          else listener = l;
        })
        .catch(() => {
          // No such plugin command: the two other routes still cover us.
        });
    }

    return () => {
      disposed = true;
      void listener?.unregister();
      delete win.__cullantBack;
      window.removeEventListener("popstate", onPopState);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  });

  // Android can launch (or re-target) the singleTask activity when the user
  // chooses Cullant for a newly attached camera, card reader or USB drive. The
  // native side retains that intent until this effect consumes it, which covers
  // both a cold launch and onNewIntent while the WebView is already alive.
  let attachedStoragePickerOpen = false;
  async function openAttachedStorage() {
    if (!IS_ANDROID || attachedStoragePickerOpen) return;
    attachedStoragePickerOpen = true;
    let handledAttach = false;
    try {
      if (!(await api.consumeUsbAttach())) return;
      handledAttach = true;
      const uri = await api.pickSafTree(true);
      if (uri) await openProject(uri);
    } finally {
      attachedStoragePickerOpen = false;
      // A second device can arrive while Android's picker is open. MainActivity
      // retains that latest intent even though the event handler above was busy;
      // consume it after this picker settles instead of dropping the attach.
      if (handledAttach) queueMicrotask(() => void openAttachedStorage());
    }
  }

  $effect(() => {
    if (!IS_ANDROID) return;
    const onUsbAttached = () => void openAttachedStorage();
    window.addEventListener("cullant:usb-attached", onUsbAttached);
    void openAttachedStorage();
    return () => window.removeEventListener("cullant:usb-attached", onUsbAttached);
  });

  // Custom window chrome is Windows-only: macOS/Linux keep native decorations
  // (restored in the Rust setup hook), and touch/mobile never gets a titlebar.
  const showTitleBar = IS_WINDOWS && !IS_TOUCH;

  $effect(() => {
    if (IS_MOBILE) return;
    const appWindow = getCurrentWindow();
    let closing = false;
    const unlisten = appWindow.onCloseRequested(async (event) => {
      event.preventDefault();
      if (closing) return;
      closing = true;
      try {
        await flushSessionSave();
        await appWindow.destroy();
      } catch (error) {
        closing = false;
        catalog.error = String(error);
      }
    });
    return () => { void unlisten.then((off) => off()); };
  });

  const hasRaws = $derived(catalog.items.some((i) => i.kind === 0));

  // The grid view differs from its defaults — colours the View toolbar button,
  // the same way active filters colour Sort & Filter. Separate mode counts:
  // it changes what every cell in the grid stands for, so now that the toggle
  // lives inside the panel this badge is the only thing left saying so.
  // Thumbnail size is deliberately not part of this. It is a comfort setting,
  // not a view that hides or regroups anything, so marking the button for it
  // would flag a state the user has nothing to undo.
  // The project name lives in the window title (OS taskbar/Alt-Tab, and the
  // custom TitleBar below) rather than the toolbar — kept out of the way
  // there, still one glance/hover away from anyone who needs the full path.
  $effect(() => {
    document.title = catalog.project ? `${catalog.project.displayName} · Cullant` : "Cullant";
  });

  const hasSubfolders = $derived(buildFolderTree(catalog.items).children.size > 0);

  const hasCustomView = $derived(
    session.groupBy.length > 0 || (!session.mirrorMode && hasRaws),
  );

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
    // A single bad verdict is not enough to put a blocking panel over the app.
    // On Android the probe resolves through SAF on the same thread the import
    // saturates, so one lost race reports "not found" for a folder that is
    // perfectly fine — and the panel sits above every dialog and swallows all
    // keys until the next tick clears it.
    let failures = 0;
    const check = async () => {
      // Nothing can move the folder mid-import, and probing while the storage
      // backend is already saturated is exactly when the answer is worthless.
      if (untrack(() => catalog.ingesting) || probing) return;
      probing = true;
      try {
        const info = await api.probeStorage(proj.rootPath);
        storageOk = info.state === "ok";
        if (info.state === "ok") {
          failures = 0;
    let disposed = false;
    let probing = false;
          wasOk = true;
          folderLostMsg = ""; // reconnected → let the user carry on
        } else if (++failures >= 2 && wasOk) {
          wasOk = false;
          folderLostMsg =
            info.state === "disconnected"
        if (disposed || catalog.project?.rootPath !== proj.rootPath) return;
              ? "The drive or volume holding this project is no longer connected. Reconnect it to keep working, or close the project."
              : "This project's folder can no longer be found. It may have been moved or deleted. Restore it, or close the project.";
        }
      } catch {
        // Ignore transient IPC errors; the next tick retries.
      }
    };
    void check();
    const id = setInterval(() => void check(), 4000);
    return () => clearInterval(id);
  });

  // Periodically rescan the open project's folder for added/removed/changed
  // files, at the interval chosen in Settings (0 = off). Only fires while the
      } finally {
        probing = false;
  // storage is reachable and no part of an import is still running, so a
  // disconnected drive or a live ingest is never piled onto. Fire-and-forget:
  // its scan:* events reconcile the catalog just like the manual triggers.
  $effect(() => {
    const minutes = settings.autoRescanMinutes;
    if (!catalog.project || minutes <= 0) return;
    const id = setInterval(
      () => {
        // `ingesting`, not `scanning`: the walk is only the first phase, and
        // piling one onto a running metadata/thumbnail pass makes the rescan
        // compete with it for the same saturated storage backend.
        if (storageOk && !catalog.ingesting) void api.rescanProject();
      },
      minutes * 60 * 1000,
    );
    return () => { disposed = true; clearInterval(id); };
  });

  async function pickProject() {
    // Android has no filesystem folder dialog, so use the SAF tree picker, which
    // returns a content:// URI. Desktop uses the native directory dialog.
    if (IS_ANDROID) {
      const uri = await api.pickSafTree();
      if (uri) await openProject(uri);
      return;
    }
    const path = await open({ directory: true, title: "Open project folder" });
    // The native dialog takes focus off the webview, and dismissing it hands
    // focus back to the OS window but not to the document. Every shortcut is a
    // window-level keydown, so cancelling the picker left the whole app deaf to
    // the keyboard until the user clicked back into the page. Restore it before
    // opening anything, so the cancel path is covered too.
    try {
      await getCurrentWindow().setFocus();
      window.focus();
    } catch {
      // Best-effort: failing to focus is not a reason to skip the open.
    }
    if (path) await openProject(path);
  }

  /** Open a project folder. Thumbnails fill in progressively, so this returns to
   *  the grid as soon as the folder's photo metadata has been read. */
  async function openProject(path: string) {
    // Reject Cullant's own sidecar dir up front (the backend guards it too, so
    // the CULLANT_OPEN_PROJECT auto-open hook is covered regardless).
    if (/(^|[\\/])\.cullant([\\/]|$)/i.test(path)) {
      catalog.error =
        "This is Cullant's own data folder (.cullant), not a photo or video folder. " +
        "Pick the folder that contains your photos or videos instead.";
      return;
    }
    await catalog.open(path);
  }

  /**
   * Wrap a toolbar click handler so the control releases DOM focus afterwards.
   * Focused buttons/selects would otherwise swallow arrow-key navigation.
   */
  function blurring(fn: () => void): (e: Event) => void {
    return (e) => {
      fn();
      if (!showSettings && !session.commitDialogOpen) (e.currentTarget as HTMLElement).blur();
    };
  }

</script>

<!-- While the project's folder is unavailable, swallow all shortcuts so the user
     can't keep culling a project whose files are gone. -->
<svelte:window
  onkeydown={(e) => folderLostMsg || handleKeydown(e)}
  oncontextmenu={(e) => e.preventDefault()}
/>

<main
  class="app"
  class:fullscreen={view.fullscreen}
  style="--bottom-bar-h: {bottomBarH}px"
>
  {#if showTitleBar}
    <TitleBar
      onOpenNew={() => void pickProject()}
      onOpenRecent={(path) => void openProject(path)}
      onCloseProject={() => (showCloseConfirm = true)}
      onRescan={() => void api.rescanProject()}
      onReimport={() => (showReimportConfirm = true)}
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
            <span class="count">{catalog.mediaCounts.photos} {catalog.mediaCounts.photos === 1 ? "file" : "files"}</span>
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
            <span class="count">{catalog.mediaCounts.videos} {catalog.mediaCounts.videos === 1 ? "file" : "files"}</span>
          </button>
        </div>
      </div>
      <div class="toolbar-center">
        <div class="segmented">
          <button class:active={view.mode === "grid"} aria-label="Grid" title={shortcutHint("Grid", "view.grid")} onclick={blurring(() => (view.mode = "grid"))}><Grid3x3 size={14} /></button>
          <button class:active={view.mode === "viewer"} aria-label="Loupe" title={shortcutHint("Loupe", "view.viewer")} onclick={blurring(() => { session.ensureFocus(); view.mode = "viewer"; })}><Eye size={14} /></button>
          <button class:active={view.mode === "compare"} aria-label="Compare" title={shortcutHint("Compare", "view.compare")} onclick={blurring(() => { session.ensureFocus(); view.mode = "compare"; })}><Columns2 size={14} /></button>
        </div>
      </div>
      <div class="toolbar-right">
        <button aria-label="Search filenames" title={shortcutHint("Search filenames", "ui.search")} class:haswork={session.nameFilter.trim() !== ""} onclick={blurring(() => (session.searchOpen = true))}>
          <Search size={14} />
        </button>
        <div class="filters-anchor">
          <button
            class:active={session.filtersPanelOpen}
            class:haswork={session.hasActiveFilters}
            title={session.hasActiveFilters
              ? "Sort & filter — filters are active"
              : "Sort & filter"}
            aria-pressed={session.hasActiveFilters}
            onclick={blurring(() => (session.filtersPanelOpen = !session.filtersPanelOpen))}
          >
            <ListFilter size={14} />
            <span>Sort &amp; Filter</span>
            <span class="sort-hint">
              <span
                >{catalog.sort === "capture"
                  ? "Date"
                  : catalog.sort === "name"
                    ? "Name"
                    : "Size"}</span
              >
              {#if catalog.sortDesc}<ArrowDown size={11} />{:else}<ArrowUp size={11} />{/if}
            </span>
          </button>
          {#if session.filtersPanelOpen}
            <FiltersPanel />
          {/if}
        </div>
        <div class="view-anchor">
          <button
            class:active={session.viewPanelOpen}
            class:haswork={hasCustomView}
            title={hasCustomView
              ? "View — settings changed. Thumbnail size, grouping and RAW+JPEG pairing."
              : "View. Thumbnail size, grouping and RAW+JPEG pairing."}
            aria-pressed={hasCustomView}
            onclick={blurring(() => (session.viewPanelOpen = !session.viewPanelOpen))}
          >
            <LayoutGrid size={14} />
            <span>View</span>
          </button>
          {#if session.viewPanelOpen}
            <GridViewPanel />
          {/if}
        </div>
        {#if view.mode !== "grid"}
          <button
            class="touchbar-toggle"
            class:active={touchBarVisible}
            title="Show/hide the action bar"
            aria-label="Show/hide the action bar"
            onclick={blurring(() => (touchBarVisible = !touchBarVisible))}
          >
            <PanelBottom size={14} />
          </button>
        {/if}
        <button title="Settings" onclick={blurring(() => (showSettings = true))}><SettingsIcon size={14} /></button>
        <button
          class="commit"
          class:haswork={session.hasCommitWork}
          title={shortcutHint(session.hasCommitWork ? "Review changes — actions are queued" : "Review changes", "commit.open")}
          onclick={blurring(() => (session.commitDialogOpen = true))}
        >
          <CheckCheck size={14} /><span>Review changes</span>
        </button>
      </div>
    </header>
    {/if}

    {#if catalog.preloading}
      <!-- The only full-screen wait left: until the walk reports back there is
           genuinely no file list to render. Everything after it (metadata,
           thumbnails, previews) fills in behind the grid's status pill. -->
      <div class="preload">
        <p class="phase">Scanning…{catalog.scanFound ? ` ${catalog.scanFound} found` : ""}</p>
        <div class="pbar indeterminate" role="progressbar" aria-label="Scanning project folder">
          <div class="pbar-fill"></div>
        </div>
      </div>
    {:else}
      {#if view.mode === "grid"}
        <div class="grid-area">
          {#if session.selectedIds.size > 0}
            <!-- Rendered as an overlay inside the (positioned) grid area, NOT in
                 flow: the bar appearing/disappearing mid-marquee would otherwise
                 shift every thumbnail, re-map the pointer's row hit-testing, and
                 oscillate the selection it just created. -->
            <SelectionBar />
          {/if}
          {#if session.folderTreeVisible}
            <FolderTree />
          {:else if hasSubfolders}
            <button
              class="tree-peek"
              title={shortcutHint("Show folder tree", "ui.toggleFolderTree")}
              aria-label="Show folder tree"
              onclick={blurring(() => (session.folderTreeVisible = true))}
            >
              <ChevronRight size={16} />
            </button>
          {/if}
          {#if session.filtered.length === 0 && catalog.items.length > 0}
            <!-- The active tab has photos/videos, but every filter combined
                 leaves nothing — a blank grid otherwise reads as a bug/empty
                 project rather than "your filters excluded everything". -->
            <div class="empty-filtered">
              <ListFilter size={28} />
              <p>No items match the current filters</p>
              <p class="filter-summary">{activeFilterSummary}</p>
              <button
                onclick={blurring(() => (session.filtersPanelOpen = true))}
              >
                <ListFilter size={13} /> Open Sort &amp; Filter
              </button>
            </div>
          {:else}
            <VirtualGrid items={session.filtered} />
          {/if}
        </div>
      {:else if view.mode === "viewer"}
        <Viewer />
      {:else if view.mode === "survey"}
        <SurveyView />
      {:else}
        <CompareView />
      {/if}
      <!-- Grid mode docks the bar as an overlay (same principle as SelectionBar:
           appearing on the first selection must not shrink the grid and jump the
           layout). Viewer/compare keep it in flow, as before. -->
      <div
        class="touchbar-dock"
        class:overlay={view.mode === "grid"}
        bind:clientHeight={bottomBarH}
      >
        <TouchActionBar
          forceShow={touchBarVisible && view.mode !== "grid"}
          hidden={(view.mode === "grid" && session.selectedIds.size === 0) || view.fullscreen}
        />
      </div>
    {/if}
  {:else}
    <div class="home" class:centered={recent.list.length === 0}>
      <div class="welcome">
        <img class="logo" src={homeLogoUrl} alt="" />
        <h1>Cullant</h1>
        <p>Fast, keyboard-first photo culling</p>
        {#if recent.list.length === 0}
          <p class="hint">Point Cullant at a folder of photos or videos to start.</p>
        {/if}
        <button class="primary" class:big={recent.list.length === 0} onclick={pickProject}>
          <!-- Custom (not lucide) so the flap is its own path: both its closed
               and hover `d` states share the same M/L/L/L/Z command structure,
               which is what lets the browser smoothly interpolate the shape
               instead of snapping — the folder visibly opens on hover. -->
          <svg
            class="folder-icon"
            width="17"
            height="17"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            <path
              class="folder-back"
              d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z"
            />
            <path class="folder-flap" d="M6 19.5L18 19.5L18 19L6 19Z" />
          </svg>
          <span>Open new project…</span>
        </button>
      </div>
      <ProjectGallery onopen={(path) => void openProject(path)} />
      <footer class="home-footer">
        <button class="opensource" onclick={() => (showSettings = true)}>
          <SettingsIcon size={13} />
          Settings
        </button>
        <button class="ghlink" onclick={() => (showSupport = true)}>
          <Heart size={15} />
          <span class="ghlink-full">Cullant is free, with no ads. Here is how to help it along.</span>
          <span class="ghlink-short">Free, no ads · Support it</span>
        </button>
      </footer>
    </div>
  {/if}

  {#if catalog.error}
    <AlertDialog title="Action failed" message={catalog.error} onclose={() => (catalog.error = "")} />
  {/if}

  {#if folderLostMsg}
    <!-- Blocking: you can't keep working while the project's folder/volume is
         gone. Reconnecting auto-dismisses this (the watcher clears the message);
         otherwise the only way out is to close the project. -->
    <div class="folder-lost" role="alertdialog" aria-modal="true" aria-label="Project folder unavailable">
      <div class="fl-panel">
        <h2>Project folder unavailable</h2>
        <p>{folderLostMsg}</p>
        <p class="fl-waiting">Waiting for it to reconnect…</p>
        <div class="fl-actions">
          <button
            class="fl-close"
            onclick={() => {
              folderLostMsg = "";
              void catalog.close();
            }}>Close project</button
          >
        </div>
      </div>
    </div>
  {/if}

  {#if view.shortcutsOpen}
    <ShortcutsOverlay onclose={() => (view.shortcutsOpen = false)} />
  {/if}

  <!-- Mounted once for the whole app: any InfoTip, in any dialog or panel,
       raises its explanation here. -->
  <InfoOverlay />

  {#if session.searchOpen}
    <SearchOverlay />
  {/if}

  {#if showKeybindings}
    <KeybindingsDialog onclose={() => leaveSubDialog(() => (showKeybindings = false))} />
  {/if}

  {#if showSettings}
    <SettingsDialog
      bind:query={settingsQuery}
      onclose={() => { showSettings = false; settingsQuery = ""; }}
      onshowkeybindings={() => fromSettings(() => (showKeybindings = true))}
      onshowtags={() => fromSettings(() => (tags.editorOpen = true))}
      onshowsupport={() => fromSettings(() => (showSupport = true))}
    />
  {/if}

  {#if showSupport}
    <SupportDialog onclose={() => leaveSubDialog(() => (showSupport = false))} />
  {/if}

  {#if session.recoupleDialogFor !== null}
    <PairSyncDialog groupId={session.recoupleDialogFor} />
  {/if}

  {#if tags.editorOpen}
    <TagEditor onclose={() => leaveSubDialog(() => (tags.editorOpen = false))} />
  {/if}

  {#if session.commitDialogOpen}
    <CommitDialog />
  {/if}

  {#if session.commitDone}
    <AlertDialog
      title={session.commitDone.title}
      message={session.commitDone.message}
      onclose={() => (session.commitDone = null)}
    />
  {/if}

  {#if catalog.xmpImported > 0}
    <!-- Shown once per scan that imported anything. Reading someone's existing
         ratings in silence would leave them wondering where the stars came
         from — or worse, not notice that Cullant now disagrees with Lightroom. -->
    <AlertDialog
      title="Ratings read from XMP"
      message={`${catalog.xmpImported} ${catalog.xmpImported === 1 ? "file" : "files"} received a rating, flag or color label from an XMP sidecar.`}
      onclose={() => (catalog.xmpImported = 0)}
    />
  {/if}

  {#if session.moveDialogOpen}
    <MoveDialog />
  {/if}

  {#if showReimportConfirm}
    <ConfirmDialog
      title="Reimport this project?"
      message="Cullant forgets everything it has stored about this project and reads the folder again from nothing: every rating, flag, colour label and tag, the queue of pending actions, and the commit history. Your photos and videos are not touched, and anything already exported to XMP sidecars comes back on the rescan."
      confirmLabel="Reimport"
      onconfirm={() => {
        showReimportConfirm = false;
        void catalog.reimport();
      }}
      oncancel={() => (showReimportConfirm = false)}
    />
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

  /* No selectable text/UI chrome app-wide (grid labels, toolbar, dialogs, …) —
     a stray text-selection highlight or drag-select is one of the things that
     makes an app read as "a web page" rather than a native tool. Real text
     entry (tag names, rename fields, the folder-path search, …) opts back in
     explicitly below; nothing else in the app needs selection. */
  :global(*) {
    user-select: none;
  }

  :global(input),
  :global(textarea),
  :global([contenteditable]) {
    user-select: text;
  }

  /* App-wide native-select restyle: strip the OS default chrome and draw our own
     control (dark fill, themed border, custom chevron). A component may still
     add layout (width/flex) via a local class, but the look lives here so every
     dropdown — group-by, filters, deletion mode, tag scope — matches. */
  :global(select) {
    appearance: none;
    -webkit-appearance: none;
    background-color: var(--control);
    background-image: var(--select-arrow);
    background-repeat: no-repeat;
    background-position: right 9px center;
    color: #e0e0e0;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    padding: 6px 28px 6px 10px;
    font-family: inherit;
    font-size: 13px;
    cursor: pointer;
  }

  @media (hover: hover) {
    :global(select:hover:not(:disabled)) {
      background-color: var(--hover);
    }
  }

  :global(select:focus-visible) {
    outline: none;
    border-color: var(--accent);
  }

  :global(select:disabled) {
    opacity: 0.5;
    cursor: default;
  }

  /* The popup list — honored on Windows/Chromium; a no-op where the OS draws
     the native menu, which is acceptable. */
  :global(select option) {
    background: var(--surface-2);
    color: #e0e0e0;
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

    /* Custom dropdown chevron for the app-wide <select> restyle above (muted
       grey, matches the theme). Kept as a var so the SVG lives in one place. */
    --select-arrow: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%238a8a93' stroke-width='2.5' stroke-linecap='round' stroke-linejoin='round'><polyline points='6 9 12 15 18 9'/></svg>");

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

    /* Side margin every centered modal dialog's max-width subtracts (one side;
       dialogs subtract it ×2). Every dialog references this ONE token instead
       of hardcoding its own margin, so the mobile-portrait override below
       widens the gutter for all of them at once. */
    --dialog-edge-margin: 12px;

    font-family: Inter, "Segoe UI", Avenir, Helvetica, Arial, sans-serif;
    font-size: 14px;
    color: #e8e8e8;
    background-color: var(--bg);
    color-scheme: dark;
  }

  /* Narrow portrait viewports (phones): the 12px default reads as the dialog
     sticking to the screen edges, so give it noticeably more breathing room.
     Width-gated (not a device check) so a narrow, tall desktop window gets the
     same treatment — the actual problem is available width, not the platform. */
  @media (max-width: 600px) and (orientation: portrait) {
    :global(:root) {
      --dialog-edge-margin: 20px;
    }
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
    /* Anchors the touch-bar overlay dock in grid mode (see .touchbar-dock). */
    position: relative;
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
    padding: calc(6px + var(--inset-top)) calc(10px + var(--safe-right)) 8px
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
    row-gap: 6px;
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
    .toolbar .touchbar-toggle {
      display: none;
    }
  }

  /* Narrow screens: left + center share the first row (flex:1 still lets
     center sit after left rather than truly mid-viewport, but there's no
     third zone competing for that row's space); the right zone claims a full
     row of its own (flex-basis 100% forces the wrap) and wraps its own
     buttons across as many further rows as it needs. `space-between` then
     spreads those buttons edge-to-edge, filling the row's free space as the
     gaps between them (with `gap` as the minimum floor) — one row when they
     fit, every wrapped row likewise balanced when they don't. */
  @media (max-width: 720px) {
    .toolbar {
      flex-wrap: wrap;
      row-gap: 6px;
    }
    .toolbar-right {
      flex: 1 1 100%;
      justify-content: space-between;
    }
    /* The Photos/Videos pill's 1px border + 2px padding inset its content, so it
       reads as ~3px right of the flush buttons that wrap onto the row below.
       Pull it back so their left edges line up. */
    .media-toggle {
      margin-left: -3px;
    }
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

  @media (hover: hover) {
    button:hover:not(:disabled),
    .toolbar button:hover:not(:disabled) {
      background-color: var(--hover);
    }
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
    /* Match the bordered "pill" controls (Photos/Videos, view mode) so every
       toolbar row is the same height and wrapped rows read evenly spaced. */
    min-height: 30px;
  }

  /* The pills are containers (border + padding), so keep their inner buttons
     shorter — the pill total (inner + 6px chrome) then equals a plain 30px
     button instead of overshooting to ~36px. */
  .media-toggle button,
  .segmented button {
    box-sizing: border-box;
    height: 24px;
    min-height: 0;
    line-height: 1;
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

  /* Colour alone carries this state otherwise, which fails anyone who cannot
     separate the two fills — and is easy to miss in a glance across the bar.
     Absolutely positioned so marking a button never changes its width and the
     toolbar cannot reflow as filters come and go. */
  .toolbar-right button.haswork {
    position: relative;
  }

  .toolbar-right button.haswork::after {
    content: "";
    position: absolute;
    top: 2px;
    right: 2px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }

  /* Anchors the Filters dropdown under its toolbar button. */
  .filters-anchor {
    position: relative;
    display: inline-flex;
  }

  /* Current sort shown right on the toolbar button, so the active order reads
     at a glance without opening the panel. */
  .sort-hint {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding-left: 6px;
    margin-left: 4px;
    border-left: 1px solid var(--border);
    opacity: 0.7;
    font-size: 11px;
  }

  /* Active-filters indicator on the Filters button. */
  .filters-anchor button.haswork {
    background: var(--accent-fill);
    border-color: transparent;
    color: #fff;
  }

  .segmented button {
    border: 1px solid transparent;
    border-radius: 6px;
    background: transparent;
    /* 3px vertical (not 4px) to match .media-btn's rhythm — with the .segmented
       container's own 2px padding + 1px border, this lands the pill at the same
       total height (38px) as every other toolbar button. */
    padding: 0 9px;
    transition: background-color 0.15s ease, color 0.15s ease;
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
    padding: 0 10px;
    font-weight: 600;
    font-size: 12px;
    transition: background-color 0.15s ease, color 0.15s ease;
  }

  .media-btn.active {
    background: var(--accent-fill);
    border-color: transparent;
    color: #fff;
    animation: pill-pop 0.15s ease;
  }

  /* The counts arrive as the scan progresses, so `0` becomes `1247` under the
     user's finger. Tabular digits plus a reserved width keep the button — and
     therefore the whole wrapping toolbar, and the popovers anchored to it —
     from reflowing while someone is aiming at it. */
  .media-btn .count {
    font-weight: 400;
    opacity: 0.65;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    display: inline-block;
    min-width: 3ch;
    text-align: right;
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
      padding-left: max(4px, var(--safe-left));
      padding-right: max(4px, var(--safe-right));
    }
  }

  .grid-area :global(.viewport) {
    flex: 1;
  }

  .empty-filtered {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 10px;
    color: #6a6a72;
    text-align: center;
    padding: 24px;
  }

  .empty-filtered p {
    margin: 0;
    font-size: 13px;
  }

  .empty-filtered button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: var(--control);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: #ddd;
    padding: 6px 12px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .empty-filtered button:hover {
    border-color: var(--accent);
    color: #fff;
  }

  /* Anchors the GridViewPanel popover under the View toolbar button. */
  .view-anchor {
    position: relative;
  }

  /* Default: transparent wrapper — the touch bar stays an in-flow flex child of
     .app (viewer/compare behavior unchanged). */
  .touchbar-dock {
    display: contents;
  }

  /* Grid mode: float the bar over the bottom of the grid instead of taking
     flow space, so the first tap-selection doesn't shrink the grid and bounce
     every thumbnail (the same jump the SelectionBar overlay fixes up top).
     The inset offsets keep it above the system nav bar / clear of cutouts,
     matching the padding .app applies to its in-flow children. */
  .touchbar-dock.overlay {
    display: block;
    position: absolute;
    left: var(--inset-left);
    right: var(--inset-right);
    bottom: var(--inset-bottom);
    z-index: 30;
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
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }

  /* On an empty homepage (no recents) the new-project button is the only thing
     to do, so give it more weight. */
  button.primary.big {
    padding: 15px 32px;
    font-size: 18px;
  }

  /* The flap's `d` transitions between two shapes built from the SAME command
     sequence (M L L L Z), so the browser can smoothly interpolate the points
     instead of jump-cutting — the front panel visibly swings down and open.
     The closed state is a thin sliver INSET well inside the back panel's own
     outline (not flush with it): the back panel's edges sit at x=2/x=22, y=20
     — a flap edge sitting right on/next to those doubles up with its 2px
     stroke and reads as one fat border (the bug). Inset + thin keeps the two
     strokes far enough apart that the closed folder shows a single clean
     outline, same as it would with no flap at all.
     The `d` SVG attribute above is only the no-hover / unsupported-browser
     fallback; this CSS property (which wins when supported) drives it. */
  .folder-flap {
    d: path("M6 19.5L18 19.5L18 19L6 19Z");
    transition: d 220ms cubic-bezier(0.34, 1.1, 0.64, 1);
  }

  button.primary:hover .folder-flap {
    d: path("M2 20.5L22 20.5L19 11L6 11Z");
  }

  .welcome .hint {
    margin: 0 0 4px;
    font-size: 12.5px;
    opacity: 0.6;
  }

  /* Blocking overlay while the open project's folder/volume is gone. */
  .folder-lost {
    position: fixed;
    inset: 0;
    z-index: 200;
    background: rgba(0, 0, 0, 0.72);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--inset-top) var(--inset-right) var(--inset-bottom) var(--inset-left);
    box-sizing: border-box;
  }

  .fl-panel {
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 18px 22px;
    width: 400px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
  }

  .fl-panel h2 {
    margin: 0 0 8px;
    font-size: 15px;
  }

  .fl-panel p {
    font-size: 13px;
    opacity: 0.8;
    margin: 0 0 6px;
  }

  .fl-waiting {
    opacity: 0.5 !important;
    font-style: italic;
  }

  .fl-actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 14px;
  }

  .fl-close {
    border-radius: 6px;
    border: 1px solid transparent;
    background: #e04343;
    color: #fff;
    padding: 8px 16px;
    font-size: 13px;
    font-family: inherit;
    cursor: pointer;
  }

  .fl-close:hover {
    filter: brightness(1.1);
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
