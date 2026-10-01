import { listen } from "@tauri-apps/api/event";
import { api, type TagChange, type TaskTag } from "../api";
import { catalog } from "./catalog.svelte";
import { session } from "./session.svelte";

class TagsStore {
  private refreshRequest = 0;
  all = $state<TaskTag[]>([]);
  editorOpen = $state(false);

  byId = $derived(new Map(this.all.map((t) => [t.id, t])));

  async refresh() {
    if (!catalog.project) return;
    const generation = catalog.generation;
    const request = ++this.refreshRequest;
    const all = await api.listTaskTags();
    if (generation !== catalog.generation || request !== this.refreshRequest) return;
    this.all = all;
    const validIds = new Set(all.map((tag) => tag.id));
    let changed = false;
    for (const item of catalog.items) {
      const tagIds = item.tagIds.filter((id) => validIds.has(id));
      if (tagIds.length !== item.tagIds.length) {
        item.tagIds = tagIds;
        changed = true;
      }
    }
    if (session.tagFilter !== null && !validIds.has(session.tagFilter)) {
      session.tagFilter = null;
      changed = true;
    }
    if (changed) session.clampFocus();
  }

  resetForNewProject() {
    this.refreshRequest += 1;
    this.all = [];
    this.editorOpen = false;
  }

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
    session.clampFocus();
  }
}

export const tags = new TagsStore();

listen<{ projectRoot: string; changes: TagChange[] }>("filetags:changed", (e) => {
  if (catalog.acceptEvent(e.payload)) tags.applyChanges(e.payload.changes);
});
listen<{ projectRoot: string }>("tags:changed", (e) => {
  if (catalog.acceptEvent(e.payload)) void tags.refresh();
});
