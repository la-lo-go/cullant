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

  /** Forget the project and delete Cullant's data (DB + thumb cache) for it.
   *  The user's photos are never touched. */
  async deleteProject(path: string) {
    await api.deleteProjectData(path);
    this.list = this.list.filter((p) => p.path !== path);
  }
}

export const recent = new RecentStore();
