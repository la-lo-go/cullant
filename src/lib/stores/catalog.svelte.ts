import { listen } from "@tauri-apps/api/event";
import { SvelteSet } from "svelte/reactivity";
import {
  api,
  type ItemLite,
  type MediaCounts,
  type MediaTab,
  type ProjectInfo,
  type ScanDone,
  type ScanProgress,
  type SortKey,
} from "../api";
import { flushSessionSave, session } from "./session.svelte";
import { view } from "./view.svelte";

class CatalogStore {
  project = $state<ProjectInfo | null>(null);
  items = $state<ItemLite[]>([]);
  sort = $state<SortKey>("capture");
  /** Descending order when true. Toggled by re-selecting the active sort. */
  sortDesc = $state(false);
  media = $state<MediaTab>("photos");
  scanning = $state(false);
  scanFound = $state(0);
  /** Photos that took their state from an XMP sidecar in the last scan; drives
   *  a one-off notice so an import is never silent. 0 = nothing to say. */
  xmpImported = $state(0);
  /** True while the folder walk still has no file list to show. Released on
   *  `scan:done`, the moment `items` is first populated — the grid then appears
   *  in mtime order and re-sorts by capture time on `metadata:done`. Everything
   *  after the walk (metadata, thumbnails, previews) reports in the grid's own
   *  status pill instead of blocking it. */
  preloading = $state(false);
  /** Background metadata read (metadata:progress); total 0 = idle. */
  metaProgress = $state({ done: 0, total: 0 });
  /** Background thumbnail pregeneration (thumbs:progress); total 0 = idle. */
  thumbProgress = $state({ done: 0, total: 0 });
  /** Background preview pregeneration (previews:progress); total 0 = idle. */
  previewProgress = $state({ done: 0, total: 0 });
  /** Background video-poster pregeneration (videos:progress); total 0 = idle.
   *  Runs last, after photo thumbnails and previews. */
  videoProgress = $state({ done: 0, total: 0 });
  /** Ids whose grid thumbnail has actually painted at least once. Lives here (not
   *  in the grid component) so it survives grid unmount/remount — returning from
   *  the loupe/compare view must not reset every cell to its loading skeleton and
   *  re-request thumbnails that already exist. Cleared when the project changes. */
  readonly thumbLoaded = new SvelteSet<number>();
  /** Ids of files that already have a generated loupe preview. Drives the
   *  per-cell "full preview still generating" spinner: an image whose grid
   *  thumbnail has painted but whose id is not in here (while the preview pass
   *  runs) is still waiting for its sharp preview. Refreshed from the backend as
   *  the background preview pass progresses; cleared when the project changes. */
  readonly previewReady = new SvelteSet<number>();
  error = $state("");
  /** Total present-file counts per kind, independent of the active tab. */
  mediaCounts = $state<MediaCounts>({ photos: 0, videos: 0 });

  /** True while any part of opening a project is still running — the walk, the
   *  metadata read, or either background artifact pass. Callers use it to keep
   *  off the storage backend while it is already saturated. */
  get ingesting() {
    return (
      this.scanning ||
      this.preloading ||
      this.metaProgress.total > 0 ||
      this.thumbProgress.total > 0 ||
      this.previewProgress.total > 0 ||
      this.videoProgress.total > 0
    );
  }

  /// Adopt a project the backend already opened (CULLANT_OPEN_PROJECT).
  async adoptCurrent() {
    const info = await api.currentProject();
    if (info) {
      this.project = info;
      this.thumbLoaded.clear();
      this.previewReady.clear();
      this.preloading = true;
      this.metaProgress = { done: 0, total: 0 };
      await this.refreshForOpen();
      // This path adopts a project the backend already opened, so the catalog is
      // populated by the line above and no scan:done of THIS session will ever
      // arrive to release the gate. There is nothing left to wait for.
      this.preloading = false;
      // Same recovery for previewReady: this path adopts a project the backend
      // already had open (no fresh scan of THIS session will emit metadata:done
      // or previews:progress to seed it), so every already-generated preview
      // would otherwise look "not ready yet" — the loupe would then always take
      // the soft-thumb + 1s-dwell path even when nothing needs generating.
      void this.refreshPreviewReady();
    }
  }

  async open(path: string) {
    this.error = "";
    // Switching projects without an explicit "Close project" first (recent-
    // projects menu, drag-drop) would otherwise abandon whatever view/filter
    // change is still sitting in the OLD project's persist debounce — flush it
    // to that project's DB before its connection goes away underneath it.
    await flushSessionSave();
    // Set the in-flight flags BEFORE awaiting openProject. The scan finishes in a
    // few milliseconds and its scan:progress/scan:done events can arrive before
    // this promise resolves; setting `scanning` after the await would then clobber
    // the listener's reset and pin the status pill at "Scanning… 0" forever.
    this.scanning = true;
    this.scanFound = 0;
    this.preloading = true;
    this.metaProgress = { done: 0, total: 0 };
    try {
      this.project = await api.openProject(path);
      this.thumbLoaded.clear();
      this.previewReady.clear();
      await this.refreshForOpen();
    } catch (e) {
      this.scanning = false;
      this.preloading = false;
      this.error = String(e);
    }
  }

  /// Pull the set of files that already have a loupe preview from the backend and
  /// merge it in (add-only: a preview never disappears mid-session). Called as
  /// the background preview pass makes progress so per-cell spinners clear.
  async refreshPreviewReady() {
    try {
      const ids = await api.previewReadyIds();
      for (const id of ids) this.previewReady.add(id);
    } catch {
      // Best-effort; a spinner just lingers until the next refresh.
    }
  }

  /// Refresh after opening a project: always land in the grid on the Photos
  /// tab, falling back to Videos for a video-only project. Then restore the
  /// remembered per-project session (sort/media/filters/focus) over the top.
  private async refreshForOpen() {
    // Guard the session-persist effect until the restore below has run, so the
    // defaults set here can never overwrite the saved blob before it loads.
    session.restoring = true;
    session.resetForNewProject();
    view.mode = "grid";
    this.media = "photos";
    await this.refresh();
    if (this.mediaCounts.photos === 0 && this.mediaCounts.videos > 0) {
      await this.setMedia("videos");
    }
    // A freshly opened project lands in the grid with nothing focused.
    session.clearFocus();
    // Always clears the `restoring` guard, even when the setting is off or no
    // saved state exists.
    await session.restoreSessionState();
    // Backfill the delete queue for files rejected before the reject/queue
    // sync existed (once per open; idempotent).
    await session.syncRejectedToQueue();
  }

  /// Apply a sort/media view remembered from a saved session, without the
  /// focus-clearing side effects of setSort/setMedia. A saved media tab is only
  /// honoured when it actually has items (else the current tab is kept).
  async applyRestoredView(sort?: SortKey, sortDesc?: boolean, media?: MediaTab) {
    if (sort) this.sort = sort;
    if (typeof sortDesc === "boolean") this.sortDesc = sortDesc;
    if (media === "videos" && this.mediaCounts.videos > 0) this.media = "videos";
    else if (media === "photos" && this.mediaCounts.photos > 0) this.media = "photos";
    await this.refresh();
  }

  async close() {
    await flushSessionSave();
    await api.closeProject();
    this.project = null;
    this.items = [];
    this.scanning = false;
    this.preloading = false;
    this.thumbLoaded.clear();
  }

  async refresh() {
    if (!this.project) return;
    this.items = await api.queryItems(this.sort, this.media, this.sortDesc);
    this.mediaCounts = await api.mediaCounts();
  }

  /// Pick a sort field; re-picking the active field flips the direction.
  async setSort(sort: SortKey) {
    if (this.sort === sort) {
      this.sortDesc = !this.sortDesc;
    } else {
      this.sort = sort;
      this.sortDesc = false;
    }
    await this.refresh();
  }

  async setMedia(media: MediaTab) {
    this.media = media;
    // Load the new tab's items BEFORE switching to the grid, so the grid never
    // flashes the previous tab's content for a frame (e.g. when switching media
    // from inside the loupe/compare view).
    await this.refresh();
    // Switching the media tab always returns to the grid with nothing focused,
    // even when invoked from inside the loupe/compare view.
    view.mode = "grid";
    session.clearFocus();
  }
}

export const catalog = new CatalogStore();
catalog.adoptCurrent();

// Backend events keep the catalog fresh while scan/metadata run.
listen<ScanProgress>("scan:progress", (e) => {
  catalog.scanning = true;
  catalog.scanFound = e.payload.found;
});
listen<ScanDone>("scan:done", async (e) => {
  catalog.scanning = false;
  // Reported once per scan, not prompted per photo: the auto-rescan interval
  // would make a dialog unbearable, but silently rewriting someone's ratings
  // would be worse.
  catalog.xmpImported = e.payload.xmpImported ?? 0;
  await catalog.refresh();
  // The file list exists now, so show it. Ordering is by mtime until the
  // metadata read finishes and re-sorts by capture time — on a camera card the
  // two orders match, and waiting for the read means staring at a progress bar
  // instead of at the photos.
  catalog.preloading = false;
});
listen<{ done: number; total: number }>("metadata:progress", (e) => {
  // Non-blocking pill. Reset to idle once the read completes.
  catalog.metaProgress =
    e.payload.done >= e.payload.total ? { done: 0, total: 0 } : e.payload;
});
listen("metadata:done", async () => {
  catalog.metaProgress = { done: 0, total: 0 };
  // Capture times are in, so re-sort. Safety net for the gate too: a project
  // opened before the scan:done listener existed still gets released here.
  catalog.preloading = false;
  await catalog.refresh();
  // Seed which items already have previews (e.g. from a previous session), so
  // items that are done never show the "generating preview" spinner.
  void catalog.refreshPreviewReady();
});
listen<{ done: number; total: number }>("thumbs:progress", (e) => {
  // Non-blocking pill. Reset to idle once the background tier completes.
  catalog.thumbProgress =
    e.payload.done >= e.payload.total ? { done: 0, total: 0 } : e.payload;
});
listen<{ total: number }>("thumbs:done", () => {
  catalog.thumbProgress = { done: 0, total: 0 };
});
listen<{ done: number; total: number; ids?: number[] }>("previews:progress", (e) => {
  const { done, total, ids } = e.payload;
  // Reset to idle once the background tier completes.
  catalog.previewProgress = done >= total ? { done: 0, total: 0 } : { done, total };
  // The event carries exactly which files finished, so their per-cell spinners
  // clear without re-reading the whole catalogue on a timer.
  for (const id of ids ?? []) catalog.previewReady.add(id);
  // One full refetch at the end still runs: it is the only thing that picks up
  // previews this pass did not generate (already cached, or made on demand).
  if (done >= total) void catalog.refreshPreviewReady();
});
listen<{ done: number; total: number }>("videos:progress", (e) => {
  // The final (video-poster) tier. Reset to idle once it completes.
  catalog.videoProgress =
    e.payload.done >= e.payload.total ? { done: 0, total: 0 } : e.payload;
});
listen<string>("scan:error", (e) => {
  catalog.scanning = false;
  // Safety valve: never leave the user trapped behind the preload panel.
  catalog.preloading = false;
  catalog.error = e.payload;
});
// The just-opened folder had no recognized photos/videos. The backend already
// rolled the whole open back (closed the project, forgot it from recents,
// removed its freshly-created .cullant sidecar) — mirror that here so the app
// lands back on the welcome screen instead of an empty, confusing grid.
listen("scan:empty", () => {
  catalog.scanning = false;
  catalog.preloading = false;
  catalog.project = null;
  catalog.error =
    "This folder doesn't contain any photos or videos Cullant recognizes. Pick a different folder.";
});
