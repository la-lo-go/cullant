import { invoke } from "@tauri-apps/api/core";

export interface ProjectInfo {
  rootPath: string;
  dbPath: string;
  schemaVersion: number;
  fileCount: number;
}

export interface RecentProject {
  path: string;
  lastOpened: number;
  available: boolean;
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
  /** Grid thumbnail could not be decoded (unsupported/corrupt source). */
  thumbFailed: boolean;
}

export interface MediaCounts {
  photos: number;
  videos: number;
}

export type SyncFrom = "raw" | "jpeg" | "none";

export interface FileMetadata {
  relPath: string;
  kind: number;
  size: number;
  width: number | null;
  height: number | null;
  captureTime: number | null;
  camera: string | null;
  lens: string | null;
  iso: number | null;
  exposureTime: string | null;
  fNumber: string | null;
  focalLength: string | null;
  exposureBias: string | null;
  flash: string | null;
  gpsLat: number | null;
  gpsLon: number | null;
}

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

export type ActionKind = "delete" | "move" | "copy";
export type PairScope = "both" | "rawonly" | "jpegonly";
export type DeletionMode = "recycle" | "permanent" | "trash";

export interface PendingAction {
  id: number;
  fileId: number;
  relPath: string;
  action: ActionKind;
  dest: string | null;
  pairToken: string | null;
  origin: number;
}

export interface CommitPlan {
  deletes: PendingAction[];
  moves: PendingAction[];
  copies: PendingAction[];
  xmpCount: number;
  deletionMode: DeletionMode;
  conflicts: string[];
  planHash: string;
}

export interface CommitOutcome {
  commitId: number;
  ok: number;
  errors: number;
  errorSamples: string[];
}

export type SortKey = "capture" | "name";
export type MediaTab = "photos" | "videos";
/** How 2560px previews are pregenerated (mirrors the backend enum). */
export type PreviewMode = "all" | "background" | "window";

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
  openProject: (path: string, previewMode?: PreviewMode) =>
    invoke<ProjectInfo>("open_project", { path, previewMode }),
  // Android SAF folder picker; returns a content:// tree URI (or null if
  // cancelled) suitable to pass to openProject. No-op returning null on desktop.
  pickSafTree: () => invoke<string | null>("pick_saf_tree"),
  currentProject: () => invoke<ProjectInfo | null>("current_project"),
  closeProject: () => invoke("close_project"),
  rescanProject: (previewMode?: PreviewMode) =>
    invoke("rescan_project", { previewMode }),
  listRecentProjects: () => invoke<RecentProject[]>("list_recent_projects"),
  removeRecentProject: (path: string) => invoke("remove_recent_project", { path }),
  queryItems: (sort: SortKey, media: MediaTab, desc: boolean) =>
    invoke<ItemLite[]>("query_items", { sort, media, desc }),
  mediaCounts: () => invoke<MediaCounts>("media_counts"),
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
  enqueueAction: (
    targets: Targets,
    action: ActionKind,
    dest: string | null,
    pairScope: PairScope
  ) => invoke<number>("enqueue_action", { targets, action, dest, pairScope }),
  removePending: (pendingIds: number[]) => invoke("remove_pending", { pendingIds }),
  clearPending: () => invoke("clear_pending"),
  listPending: () => invoke<PendingAction[]>("list_pending"),
  commitPreview: () => invoke<CommitPlan>("commit_preview"),
  commitExecute: (planHash: string) => invoke<CommitOutcome>("commit_execute", { planHash }),
  getProjectSetting: (key: string) => invoke<string | null>("get_project_setting", { key }),
  setProjectSetting: (key: string, value: string) =>
    invoke("set_project_setting", { key, value }),
  getFileMetadata: (fileId: number) =>
    invoke<FileMetadata>("get_file_metadata", { fileId }),
};

// The cullant:// scheme is served as http://cullant.localhost/ on Windows and
// Android (WebView2 / Android WebView rewrite), and as cullant://localhost/ on
// macOS/iOS/Linux.
const CULLANT_BASE =
  navigator.userAgent.includes("Windows") || navigator.userAgent.includes("Android")
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

export function videoUrl(item: ItemLite): string {
  return cullantUrl(`video/${item.id}?v=${item.mtime}`);
}

/** Preview thumbnail for a homepage recent-project card (0-based list index + preview slot). */
export function recentThumbUrl(index: number, slot: number): string {
  return cullantUrl(`recent-thumb/${index}/${slot}`);
}
