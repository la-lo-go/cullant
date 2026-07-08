import { invoke } from "@tauri-apps/api/core";

export interface ProjectInfo {
  rootPath: string;
  dbPath: string;
  schemaVersion: number;
  fileCount: number;
}

export interface ItemLite {
  id: number;
  groupId: number;
  kind: number; // 0=raw 1=image 2=video
  relPath: string;
  name: string;
  ext: string;
  mtime: number;
  captureTime: number | null;
  rating: number;
  flag: number; // -1 reject, 0 unflagged, 1 pick
  label: string | null;
  width: number | null;
  height: number | null;
  isPrimary: boolean;
  groupSize: number;
  decoupled: boolean;
  tagIds: number[];
}

export type SyncFrom = "raw" | "jpeg" | "none";

export interface TaskTag {
  id: number;
  name: string;
  shortcut: string | null;
  scope: number; // 0=photo 1=video 2=both
  color: string | null;
  sortOrder: number;
  builtin: boolean;
}

export interface TagChange {
  fileId: number;
  tagId: number;
  tagged: boolean;
}

export type SortKey = "capture" | "name";
export type MediaTab = "photos" | "videos";

export interface ScanProgress {
  found: number;
}
export interface ScanDone {
  fileCount: number;
  newFiles: number;
  missingFiles: number;
}

export interface Targets {
  ids: number[];
  asGroups: boolean;
}

export interface CullState {
  id: number;
  rating: number;
  flag: number;
  label: string | null;
}

export const api = {
  openProject: (path: string) => invoke<ProjectInfo>("open_project", { path }),
  currentProject: () => invoke<ProjectInfo | null>("current_project"),
  closeProject: () => invoke("close_project"),
  rescanProject: () => invoke("rescan_project"),
  queryItems: (sort: SortKey, media: MediaTab) =>
    invoke<ItemLite[]>("query_items", { sort, media }),
  setRating: (targets: Targets, rating: number) =>
    invoke<CullState[]>("set_rating", { targets, rating }),
  setFlag: (targets: Targets, flag: number) =>
    invoke<CullState[]>("set_flag", { targets, flag }),
  setLabel: (targets: Targets, label: string | null) =>
    invoke<CullState[]>("set_label", { targets, label }),
  decoupleGroup: (groupId: number) => invoke("decouple_group", { groupId }),
  recoupleGroup: (groupId: number, syncFrom: SyncFrom) =>
    invoke("recouple_group", { groupId, syncFrom }),
  listTaskTags: () => invoke<TaskTag[]>("list_task_tags"),
  createTaskTag: (name: string, shortcut: string | null, scope: number, color: string | null) =>
    invoke<TaskTag>("create_task_tag", { name, shortcut, scope, color }),
  updateTaskTag: (tag: TaskTag) => invoke("update_task_tag", { tag }),
  deleteTaskTag: (tagId: number) => invoke("delete_task_tag", { tagId }),
  toggleTaskTag: (targets: Targets, tagId: number) =>
    invoke<TagChange[]>("toggle_task_tag", { targets, tagId }),
};

// The cullant:// scheme is served as http://cullant.localhost/ on Windows.
const CULLANT_BASE = navigator.userAgent.includes("Windows")
  ? "http://cullant.localhost/"
  : "cullant://localhost/";

export function cullantUrl(path: string): string {
  return CULLANT_BASE + path;
}

export function thumbUrl(item: ItemLite): string {
  return cullantUrl(`thumb/${item.id}?v=${item.mtime}`);
}

export function previewUrl(item: ItemLite): string {
  return cullantUrl(`preview/${item.id}?v=${item.mtime}`);
}
