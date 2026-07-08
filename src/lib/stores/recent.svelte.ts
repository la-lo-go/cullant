import { api, type RecentProject } from "../api";

class RecentStore {
  list = $state<RecentProject[]>([]);
  loaded = $state(false);

  async refresh() {
    this.list = await api.listRecentProjects();
    this.loaded = true;
  }

  async remove(path: string) {
    await api.removeRecentProject(path);
    this.list = this.list.filter((p) => p.path !== path);
  }
}

export const recent = new RecentStore();
