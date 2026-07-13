import { listen } from "@tauri-apps/api/event";
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
import { session } from "./session.svelte";
import { settings } from "./settings.svelte";
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
  /** True while the initial scan + thumbnail preload blocks the grid. */
  preloading = $state(false);
  thumbProgress = $state({ done: 0, total: 0 });
  /** Background preview pregeneration (previews:progress); total 0 = idle. */
  previewProgress = $state({ done: 0, total: 0 });
  error = $state("");
  /** Total present-file counts per kind, independent of the active tab. */
  mediaCounts = $state<MediaCounts>({ photos: 0, videos: 0 });

  /// Adopt a project the backend already opened (CULLANT_OPEN_PROJECT).
  async adoptCurrent() {
    const info = await api.currentProject();
    if (info) {
      this.project = info;
      this.preloading = true;
      this.thumbProgress = { done: 0, total: 0 };
      await this.refreshForOpen();
    }
  }

  async open(path: string) {
    this.error = "";
    try {
      this.project = await api.openProject(path, settings.previewMode);
      this.scanning = true;
      this.scanFound = 0;
      this.preloading = true;
      this.thumbProgress = { done: 0, total: 0 };
      await this.refreshForOpen();
    } catch (e) {
      this.error = String(e);
    }
  }

  /// Refresh after opening a project: always land in the grid on the Photos
  /// tab, falling back to Videos for a video-only project. Then restore the
  /// remembered per-project session (sort/media/filters/focus) over the top.
  private async refreshForOpen() {
    // Guard the session-persist effect until the restore below has run, so the
    // defaults set here can never overwrite the saved blob before it loads.
    session.restoring = true;
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
    await api.closeProject();
    this.project = null;
    this.items = [];
    this.scanning = false;
    this.preloading = false;
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
listen<ScanDone>("scan:done", async () => {
  catalog.scanning = false;
  await catalog.refresh();
});
listen("metadata:done", async () => {
  await catalog.refresh();
});
listen<{ done: number; total: number }>("thumbs:progress", (e) => {
  catalog.thumbProgress = e.payload;
});
listen<{ done: number; total: number }>("previews:progress", (e) => {
  // Reset to idle once the background tier completes.
  catalog.previewProgress =
    e.payload.done >= e.payload.total ? { done: 0, total: 0 } : e.payload;
});
listen<{ total: number }>("thumbs:done", async (e) => {
  catalog.thumbProgress = { done: e.payload.total, total: e.payload.total };
  catalog.preloading = false;
  await catalog.refresh();
});
listen<string>("scan:error", (e) => {
  catalog.scanning = false;
  // Safety valve: never leave the user trapped behind the preload panel.
  catalog.preloading = false;
  catalog.error = e.payload;
});
