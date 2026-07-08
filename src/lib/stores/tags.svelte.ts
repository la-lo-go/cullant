import { listen } from "@tauri-apps/api/event";
import { api, type TagChange, type TaskTag } from "../api";
import { catalog } from "./catalog.svelte";

class TagsStore {
  all = $state<TaskTag[]>([]);
  editorOpen = $state(false);

  byId = $derived(new Map(this.all.map((t) => [t.id, t])));

  async refresh() {
    if (!catalog.project) return;
    this.all = await api.listTaskTags();
  }

  /** Apply toggle results to the in-memory catalog. */
  applyChanges(changes: TagChange[]) {
    if (changes.length === 0) return;
    const byFile = new Map<number, TagChange[]>();
    for (const c of changes) {
      const list = byFile.get(c.fileId);
      if (list) list.push(c);
      else byFile.set(c.fileId, [c]);
    }
    for (const item of catalog.items) {
      const list = byFile.get(item.id);
      if (!list) continue;
      for (const c of list) {
        if (c.tagged && !item.tagIds.includes(c.tagId)) {
          item.tagIds = [...item.tagIds, c.tagId];
        } else if (!c.tagged) {
          item.tagIds = item.tagIds.filter((t) => t !== c.tagId);
        }
      }
    }
  }
}

export const tags = new TagsStore();

listen<TagChange[]>("filetags:changed", (e) => tags.applyChanges(e.payload));
listen("tags:changed", () => tags.refresh());
