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
      await this.refresh();
    }
  }

  async open(path: string) {
    this.error = "";
    try {
      this.project = await api.openProject(path);
      this.scanning = true;
      this.scanFound = 0;
      this.preloading = true;
      this.thumbProgress = { done: 0, total: 0 };
      await this.refresh();
    } catch (e) {
      this.error = String(e);
    }
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
    await this.refresh();
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
