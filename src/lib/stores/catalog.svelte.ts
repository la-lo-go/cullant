import { listen } from "@tauri-apps/api/event";
import { SvelteSet } from "svelte/reactivity";
import {
  api,
  setMediaProjectRoot,
  type ItemLite,
  type MediaCounts,
  type MediaTab,
  type ProjectInfo,
  type ScanDone,
  type ScanProgress,
  type SortKey,
} from "../api";
import { flushSessionSave, session } from "./session.svelte";
import { tags } from "./tags.svelte";
import { view } from "./view.svelte";
import { folders } from "./folders.svelte";
import { settings } from "./settings.svelte";

class CatalogStore {
  generation = 0;
  private refreshRequest = 0;
  private previewRequest = 0;
  private previewDuringRefresh = new Set<number>();
  private previewPass = 0;
  private previewPassComplete = false;
  private eventRoot: string | null = null;
  private transitionTail: Promise<void> = Promise.resolve();

  private transition(run: () => Promise<void>): Promise<void> {
    const task = this.transitionTail.then(run);
    this.transitionTail = task.catch(() => {});
    return task;
  }
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
  readonly previewUnavailable = new SvelteSet<number>();
  error = $state("");
  /** Total present-file counts per kind, independent of the active tab. */
  mediaCounts = $state<MediaCounts>({ photos: 0, videos: 0 });

  acceptEvent(payload: { projectRoot?: string } | null | undefined): boolean {
    return !!this.eventRoot && payload?.projectRoot === this.eventRoot;
  }

  private resetProjectState() {
    this.items = [];
    this.error = "";
    this.media = "photos";
    this.sort = "capture";
    this.sortDesc = false;
    this.scanFound = 0;
    this.xmpImported = 0;
    this.metaProgress = { done: 0, total: 0 };
    this.thumbProgress = { done: 0, total: 0 };
    this.previewProgress = { done: 0, total: 0 };
    this.videoProgress = { done: 0, total: 0 };
    this.mediaCounts = { photos: 0, videos: 0 };
    this.thumbLoaded.clear();
    this.invalidatePreviewReady();
    session.resetForNewProject();
    folders.reset();
    tags.resetForNewProject();
  }

  private async recoverOpen(error: unknown, generation: number) {
    if (generation !== this.generation) return;
    this.scanning = false;
    this.preloading = false;
    try {
      const project = await api.currentProject();
      if (generation !== this.generation) return;
      this.project = project;
      this.eventRoot = project?.rootPath ?? null;
      setMediaProjectRoot(this.eventRoot);
      if (project) await this.refreshForOpen(generation);
    } catch {
      // Preserve the transition error if the recovery read fails.
    }
    if (generation !== this.generation) return;
    session.restoring = false;
    this.error = String(error);
  }

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
    const generation = this.generation;
    const info = await api.currentProject();
    if (generation !== this.generation) return;
    if (info) {
      this.eventRoot = info.rootPath;
      setMediaProjectRoot(info.rootPath);
      this.project = info;
      this.thumbLoaded.clear();
      this.invalidatePreviewReady();
      this.preloading = true;
      this.metaProgress = { done: 0, total: 0 };
      await this.refreshForOpen(generation);
      if (generation !== this.generation) return;
      // This path adopts a project the backend already opened, so the catalog is
      // populated by the line above and no scan:done of THIS session will ever
      // arrive to release the gate. There is nothing left to wait for.
      this.preloading = false;
    }
  }

  open(path: string) {
    return this.transition(() => this.openNow(path));
  }

  private async openNow(path: string) {
    this.error = "";
    // Switching projects without an explicit "Close project" first (recent-
    // projects menu, drag-drop) would otherwise abandon whatever view/filter
    // change is still sitting in the OLD project's persist debounce — flush it
    // to that project's DB before its connection goes away underneath it.
    await flushSessionSave();
    const generation = ++this.generation;
    this.eventRoot = path;
    session.restoring = true;
    this.resetProjectState();
    // Set the in-flight flags BEFORE awaiting openProject. The scan finishes in a
    // few milliseconds and its scan:progress/scan:done events can arrive before
    // this promise resolves; setting `scanning` after the await would then clobber
    // the listener's reset and pin the status pill at "Scanning… 0" forever.
    this.scanning = true;
    this.scanFound = 0;
    this.preloading = true;
    this.metaProgress = { done: 0, total: 0 };
    try {
      const project = await api.openProject(path);
      if (generation !== this.generation) return;
      this.eventRoot = project.rootPath;
      setMediaProjectRoot(project.rootPath);
      this.project = project;
      await this.refreshForOpen(generation);
    } catch (e) {
      await this.recoverOpen(e, generation);
    }
  }

  /**
   * Throw away everything stored about the open project and read the folder
   * again from nothing.
   *
   * Same shape as `open`, because from here it IS an open: the backend wipes its
   * data directory and reopens the same folder in one step, and the frontend has
   * the same job afterwards — clear the caches keyed on file ids that no longer
   * exist, and refresh as if the project had just been picked.
   */
  reimport() {
    return this.transition(() => this.reimportNow());
  }

  private async reimportNow() {
    this.error = "";
    await flushSessionSave();
    const generation = ++this.generation;
    this.eventRoot = this.project?.rootPath ?? null;
    session.restoring = true;
    this.resetProjectState();
    this.scanning = true;
    this.scanFound = 0;
    this.preloading = true;
    this.metaProgress = { done: 0, total: 0 };
    try {
      const project = await api.reimportProject();
      if (generation !== this.generation) return;
      this.eventRoot = project.rootPath;
      setMediaProjectRoot(project.rootPath);
      this.project = project;
      await this.refreshForOpen(generation);
    } catch (e) {
      await this.recoverOpen(e, generation);
    }
  }

  resetPreviewPass() {
    this.previewPass++;
    this.previewPassComplete = false;
    this.previewUnavailable.clear();
  }

  invalidatePreviewReady() {
    this.previewRequest++;
    this.previewDuringRefresh.clear();
    this.previewReady.clear();
    this.resetPreviewPass();
  }

  markPreviewReady(ids: number[]) {
    for (const id of ids) {
      this.previewReady.add(id);
      this.previewDuringRefresh.add(id);
      this.previewUnavailable.delete(id);
    }
  }

  markPreviewUnavailable(id: number) {
    this.previewReady.delete(id);
    this.previewDuringRefresh.delete(id);
    this.previewUnavailable.add(id);
  }

  private updatePreviewUnavailable() {
    if (!this.previewPassComplete) return;
    for (const item of this.items) {
      if (item.kind === 2 || (!item.isPrimary && !item.decoupled)) continue;
      if (
        !item.thumbFailed && !item.previewFailed && !this.previewReady.has(item.id)
      ) this.previewUnavailable.add(item.id);
      else this.previewUnavailable.delete(item.id);
    }
  }

  /// Replace stale readiness. Keep previews completed after the query started.
  async refreshPreviewReady() {
    const generation = this.generation;
    const request = ++this.previewRequest;
    const quality = settings.previewQuality;
    const completed = new Set<number>();
    this.previewDuringRefresh = completed;
    try {
      const ids = await api.previewReadyIds();
      if (
        generation !== this.generation || request !== this.previewRequest ||
        quality !== settings.previewQuality
      ) return false;
      this.previewReady.clear();
      for (const id of ids) this.previewReady.add(id);
      for (const id of completed) this.previewReady.add(id);
      this.updatePreviewUnavailable();
      return true;
    } catch {
      // Best-effort; a spinner just lingers until the next refresh.
      return false;
    }
  }

  async finishPreviewPass() {
    const generation = this.generation;
    const pass = this.previewPass;
    const quality = settings.previewQuality;
    await this.refresh();
    if (
      generation !== this.generation || pass !== this.previewPass ||
      quality !== settings.previewQuality || this.scanning ||
      this.metaProgress.total > 0 || this.thumbProgress.total > 0 ||
      this.previewProgress.total > 0
    ) return;
    this.previewPassComplete = true;
    await this.refreshPreviewReady();
  }

  /// Refresh after opening a project: always land in the grid on the Photos
  /// tab, falling back to Videos for a video-only project. Then restore the
  /// remembered per-project session (sort/media/filters/focus) over the top.
  private async refreshForOpen(generation: number) {
    // Guard the session-persist effect until the restore below has run, so the
    // defaults set here can never overwrite the saved blob before it loads.
    session.restoring = true;
    session.resetForNewProject();
    view.mode = "grid";
    this.media = "photos";
    await folders.restore();
    if (generation !== this.generation) return;
    await this.refresh();
    if (generation !== this.generation) return;
    await this.refreshPreviewReady();
    if (generation !== this.generation) return;
    if (this.mediaCounts.photos === 0 && this.mediaCounts.videos > 0) {
      await this.setMedia("videos");
      if (generation !== this.generation) return;
    }
    // A freshly opened project lands in the grid with nothing focused.
    session.clearFocus();
    // Always clears the `restoring` guard, even when the setting is off or no
    // saved state exists.
    await session.restoreSessionState();
    if (generation !== this.generation) return;
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

  close() {
    return this.transition(() => this.closeNow());
  }

  private async closeNow() {
    await flushSessionSave();
    const generation = ++this.generation;
    this.eventRoot = null;
    try {
      await api.closeProject();
    } catch (error) {
      if (generation === this.generation) {
        this.eventRoot = this.project?.rootPath ?? null;
        this.error = String(error);
      }
      return;
    }
    if (generation !== this.generation) return;
    this.project = null;
    setMediaProjectRoot(null);
    this.resetProjectState();
    this.scanning = false;
    this.preloading = false;
    this.thumbLoaded.clear();
    // File ids are unique only inside one project's database, so neither set may
    // outlive the project that filled it — the same invariant close_project
    // keeps on the backend when it clears the thumbnail memory cache.
    this.previewReady.clear();
  }

  async refresh() {
    if (!this.project) return;
    const generation = this.generation;
    const request = ++this.refreshRequest;
    const [items, counts] = await Promise.all([
      api.queryItems(this.sort, this.media, this.sortDesc),
      api.mediaCounts(),
    ]);
    if (generation !== this.generation || request !== this.refreshRequest) return;
    this.items = items;
    this.mediaCounts = counts;
    this.updatePreviewUnavailable();
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
    const generation = this.generation;
    this.media = media;
    // Load the new tab's items BEFORE switching to the grid, so the grid never
    // flashes the previous tab's content for a frame (e.g. when switching media
    // from inside the loupe/compare view).
    await this.refresh();
    if (generation !== this.generation || media !== this.media) return;
    // Switching the media tab always returns to the grid with nothing focused,
    // even when invoked from inside the loupe/compare view.
    view.mode = "grid";
    session.clearFocus();
  }
}

export const catalog = new CatalogStore();
void catalog.adoptCurrent().catch((error) => { catalog.error = String(error); });

listen<ScanProgress>("scan:progress", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  if (!catalog.scanning) catalog.resetPreviewPass();
  catalog.scanning = true;
  catalog.scanFound = e.payload.found;
});
listen<ScanDone>("scan:done", async (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  const generation = catalog.generation;
  catalog.resetPreviewPass();
  catalog.scanning = false;
  // Reported once per scan, not prompted per photo: the auto-rescan interval
  // would make a dialog unbearable, but silently rewriting someone's ratings
  // would be worse.
  catalog.xmpImported = e.payload.xmpImported ?? 0;
  await catalog.refresh();
  if (generation !== catalog.generation) return;
  // The file list exists now, so show it. Ordering is by mtime until the
  // metadata read finishes and re-sorts by capture time — on a camera card the
  // two orders match, and waiting for the read means staring at a progress bar
  // instead of at the photos.
  catalog.preloading = false;
});
listen<{ projectRoot: string; done: number; total: number }>("metadata:progress", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  catalog.metaProgress =
    e.payload.done >= e.payload.total ? { done: 0, total: 0 } : e.payload;
});
listen<{ projectRoot: string }>("metadata:done", async (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  const generation = catalog.generation;
  catalog.metaProgress = { done: 0, total: 0 };
  // Capture times are in, so re-sort. Safety net for the gate too: a project
  // opened before the scan:done listener existed still gets released here.
  catalog.preloading = false;
  await catalog.refresh();
  if (generation !== catalog.generation) return;
  // Seed which items already have previews (e.g. from a previous session), so
  // items that are done never show the "generating preview" spinner.
  void catalog.refreshPreviewReady();
});
listen<{ projectRoot: string; done: number; total: number }>("thumbs:progress", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  catalog.thumbProgress =
    e.payload.done >= e.payload.total ? { done: 0, total: 0 } : e.payload;
});
listen<{ projectRoot: string; total: number }>("thumbs:done", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  catalog.thumbProgress = { done: 0, total: 0 };
});
listen<{ projectRoot: string; done: number; total: number; ids?: number[] }>("previews:progress", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  const { done, total, ids } = e.payload;
  catalog.previewProgress = done >= total ? { done: 0, total: 0 } : { done, total };
  // The event carries exactly which files finished, so their per-cell spinners
  // clear without re-reading the whole catalogue on a timer.
  catalog.markPreviewReady(ids ?? []);
  if (done >= total) {
    // One full refetch at the end: the only thing that picks up previews this
    // pass did not generate (already cached, or made on demand).
    // And re-read the catalogue itself. Its rows were snapshotted before any of
    // this ran, so thumbReady, previewFailed and the perceptual hashes that
    // group bursts are all as they were at open — which is most visibly wrong
    // after reopening a half-finished import, where a lot was generated this
    // pass. Once per import, not per tick.
    void catalog.finishPreviewPass().catch((error) => { catalog.error = String(error); });
  }
});
listen<{ projectRoot: string; done: number; total: number }>("videos:progress", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  catalog.videoProgress =
    e.payload.done >= e.payload.total ? { done: 0, total: 0 } : e.payload;
});
listen<{ projectRoot: string; error: string }>("scan:error", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  catalog.scanning = false;
  catalog.resetPreviewPass();
  catalog.metaProgress = { done: 0, total: 0 };
  catalog.thumbProgress = { done: 0, total: 0 };
  catalog.previewProgress = { done: 0, total: 0 };
  catalog.videoProgress = { done: 0, total: 0 };
  // Never leave the preload panel blocking the UI after a scan failure.
  catalog.preloading = false;
  catalog.error = e.payload.error;
});
// The just-opened folder had no recognized photos/videos. The backend already
// rolled the whole open back (closed the project, forgot it from recents,
// removed its freshly-created .cullant sidecar) — mirror that here so the app
// lands back on the welcome screen instead of an empty, confusing grid.
listen<{ projectRoot: string }>("scan:empty", (e) => {
  if (!catalog.acceptEvent(e.payload)) return;
  catalog.scanning = false;
  catalog.preloading = false;
  catalog.project = null;
  catalog.error =
    "This folder doesn't contain any photos or videos Cullant recognizes. Pick a different folder.";
});
