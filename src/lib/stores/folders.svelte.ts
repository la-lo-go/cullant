import { SvelteSet } from "svelte/reactivity";
import { api } from "../api";
import { catalog } from "./catalog.svelte";

export function folderContains(parent: string, path: string): boolean {
  return path === parent || path.startsWith(`${parent}/`);
}

function validPath(path: unknown): path is string {
  return typeof path === "string" && !path.includes("\\") &&
    path.split("/").every((part) => part !== "" && part !== "." && part !== "..");
}

function savedPaths(raw: string | null): string[] {
  const paths: unknown = JSON.parse(raw ?? "[]");
  if (!Array.isArray(paths)) return [];
  return [...new Set(paths.filter(validPath))];
}

function selectionRule(scope: Record<string, boolean>, path: string): string | undefined {
  let parent = path;
  while (parent !== "") {
    if (typeof scope[parent] === "boolean") return parent;
    const separator = parent.lastIndexOf("/");
    parent = separator < 0 ? "" : parent.slice(0, separator);
  }
  return typeof scope[""] === "boolean" ? "" : undefined;
}

function scopeIncludes(scope: Record<string, boolean>, path: string): boolean {
  const rule = selectionRule(scope, path);
  return rule !== undefined && scope[rule];
}

class FolderStore {
  collapsed = new SvelteSet<string>();
  // The nearest ancestor rule controls each subtree. The empty path means All.
  scope = $state<Record<string, boolean>>({ "": true });
  anchor = $state<string | null>(null);
  allSelected = $derived(this.scope[""] === true && Object.keys(this.scope).length === 1);
  ignored = $state<string[]>([]);
  saving = $state(false);

  reset() {
    this.collapsed.clear();
    this.selectOnly(null);
    this.ignored = [];
    this.saving = false;
  }

  ignoredBy(path: string): string | undefined {
    return this.ignored.find((parent) => folderContains(parent, path));
  }

  includes(path: string): boolean {
    return scopeIncludes(this.scope, path);
  }

  selectOnly(path: string | null) {
    this.scope = { [path ?? ""]: true };
    this.anchor = path;
  }

  select(path: string, modifiers: { ctrlKey: boolean; metaKey: boolean; shiftKey: boolean }, visiblePaths: string[]) {
    const additive = modifiers.ctrlKey || modifiers.metaKey;
    const from = this.anchor === null ? -1 : visiblePaths.indexOf(this.anchor);
    const to = visiblePaths.indexOf(path);
    if (modifiers.shiftKey && from >= 0 && to >= 0) {
      const range = visiblePaths.slice(Math.min(from, to), Math.max(from, to) + 1);
      if (!additive) this.scope = {};
      this.setSubtrees(range.filter((folder) => this.ignoredBy(folder) === undefined), true);
    } else if (additive) {
      this.setSubtrees([path], !this.includes(path));
      this.anchor = path;
    } else {
      this.selectOnly(path);
    }
  }

  private setSubtrees(paths: string[], selected: boolean) {
    let next = { ...this.scope };
    for (const path of paths) {
      for (const existing of Object.keys(next)) if (folderContains(path, existing)) delete next[existing];
      if (scopeIncludes(next, path) !== selected) next = { ...next, [path]: selected };
    }
    this.scope = next;
  }

  restoreScope(value: unknown) {
    if (!value || typeof value !== "object" || Array.isArray(value)) return;
    const entries = Object.entries(value);
    if (!entries.every(([path, selected]) => (path === "" || validPath(path)) && typeof selected === "boolean")) return;
    this.scope = Object.fromEntries(entries);
    this.anchor = null;
  }

  reconcileScope(paths: Set<string>) {
    const entries = Object.entries(this.scope);
    const kept = entries.filter(([path]) => path === "" || paths.has(path));
    if (kept.length !== entries.length) {
      this.scope = kept.some(([, selected]) => selected) ? Object.fromEntries(kept) : { "": true };
    }
    if (this.anchor !== null && !paths.has(this.anchor)) this.anchor = null;
  }

  toggleCollapsed(path: string) {
    if (this.collapsed.has(path)) this.collapsed.delete(path);
    else this.collapsed.add(path);
  }

  async restore() {
    const generation = catalog.generation;
    try {
      const raw = await api.getProjectSetting("ignoredFolders");
      if (generation === catalog.generation) this.ignored = savedPaths(raw);
    } catch (error) {
      if (generation === catalog.generation) catalog.error = `Could not load ignored folders: ${error}`;
    }
  }

  async setIgnored(path: string, ignored: boolean) {
    const next = ignored ? [...new Set([...this.ignored, path])] : this.ignored.filter((folder) => folder !== path);
    return this.saveIgnored(next);
  }

  async showAll() {
    return this.saveIgnored([]);
  }

  private async saveIgnored(next: string[]): Promise<boolean> {
    if (this.saving || !catalog.project) return false;
    const generation = catalog.generation;
    this.saving = true;
    try {
      await api.setProjectSetting("ignoredFolders", JSON.stringify(next));
      if (generation !== catalog.generation) return false;
      this.ignored = next;
      return true;
    } catch (error) {
      if (generation === catalog.generation) catalog.error = `Could not save ignored folders: ${error}`;
      return false;
    } finally {
      if (generation === catalog.generation) this.saving = false;
    }
  }
}

export const folders = new FolderStore();
