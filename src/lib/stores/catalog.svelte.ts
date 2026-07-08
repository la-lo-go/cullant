import { listen } from "@tauri-apps/api/event";
import {
  api,
  type ItemLite,
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
  media = $state<MediaTab>("photos");
  scanning = $state(false);
  scanFound = $state(0);
  error = $state("");

  /// Adopt a project the backend already opened (CULLANT_OPEN_PROJECT).
  async adoptCurrent() {
    const info = await api.currentProject();
    if (info) {
      this.project = info;
      await this.refresh();
    }
  }

  async open(path: string) {
    this.error = "";
    try {
      this.project = await api.openProject(path);
      this.scanning = true;
      this.scanFound = 0;
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
  }

  async refresh() {
    if (!this.project) return;
    this.items = await api.queryItems(this.sort, this.media);
  }

  async setSort(sort: SortKey) {
    this.sort = sort;
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
listen<string>("scan:error", (e) => {
  catalog.scanning = false;
  catalog.error = e.payload;
});
