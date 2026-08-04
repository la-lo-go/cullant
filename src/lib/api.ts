import { invoke } from "@tauri-apps/api/core";

export interface ProjectInfo {
  rootPath: string;
  /** Friendly, human-readable project name derived from `rootPath` (leaf
   *  folder name on desktop, decoded label for Android SAF content:// URIs). */
  displayName: string;
  dbPath: string;
  schemaVersion: number;
  fileCount: number;
}

/** Whether a remembered project's folder is reachable, and if not, why. */
export type StorageState = "ok" | "disconnected" | "notFound";
/** Coarse storage classification, for the gallery badge. */
export type StorageKind = "internal" | "removable" | "network" | "unknown";

export interface StorageInfo {
  state: StorageState;
  kind: StorageKind;
  /** Friendly volume name (drive letter/label, "SD card", …), if known. */
  volumeName: string | null;
}

export interface RecentProject {
  path: string;
  /** Friendly, human-readable name for display (see `ProjectInfo.displayName`). */
  displayName: string;
  lastOpened: number;
  /** Back-compat: true iff `storage.state === "ok"`. */
  available: boolean;
  storage: StorageInfo;
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
  /** Raw EXIF orientation (1-8) or null. Values 5-8 mean the displayed image is
   *  rotated 90°, so displayed dims are the swap of `width`/`height` (which are
   *  the un-rotated sensor dims and are never swapped by the backend). */
  orientation: number | null;
  /** Photographic-settings facets for the filter panel. Null on videos and on
   *  images missing the relevant EXIF tag. `camera` is "Make Model"; `lens` is
   *  the lens model; `focalLength` is in mm; `fNumber` is the bare aperture
   *  value (2.8 for f/2.8). */
  camera: string | null;
  lens: string | null;
  iso: number | null;
  focalLength: number | null;
  fNumber: number | null;
  /** Shutter speed (exposure time) in seconds. */
  exposureTime: number | null;
  isPrimary: boolean;
  groupSize: number;
  decoupled: boolean;
  tagIds: number[];
  /** Grid thumbnail could not be decoded (unsupported/corrupt source). */
  thumbFailed: boolean;
  /** A usable grid thumbnail already exists on disk, so the cell is waiting on
   *  a fetch rather than on generation. */
  thumbReady: boolean;
  /** The loupe preview was tried and could not be produced — the "generating"
   *  spinner must stop for this photo. */
  previewFailed: boolean;
  /** 16-char hex of the thumbnail's perceptual hash, or null until that
   *  thumbnail has been generated. Used to tell burst frames apart. */
  phash: string | null;
}

/** An item's DISPLAYED (post-EXIF-rotation) pixel dimensions, or `null` when
 *  unknown. width/height are un-rotated sensor dims; EXIF orientation 5-8
 *  means the shown image is turned 90°, so displayed w/h are swapped. */
export function displayDims(item: ItemLite): { w: number; h: number } | null {
  if (item.width == null || item.height == null) return null;
  const rotated = item.orientation != null && item.orientation >= 5 && item.orientation <= 8;
  return rotated ? { w: item.height, h: item.width } : { w: item.width, h: item.height };
}

/** The rendered size of a `dims`-ratio image `object-fit: contain`-ed into a
 *  `maxW × maxH` box (same math the browser uses) — the exact box any
 *  overlay/badge anchored to "the actual photo" must be sized to, whenever
 *  BOTH box axes are free to be the binding constraint (a fixed-height square
 *  cell only ever has one free axis; a fixed-WIDTH, variable-height cell like
 *  the filmstrip can bind on either). Falls back to filling the whole box when
 *  `dims` is unknown (matches plain `object-fit: contain`'s behavior absent
 *  any other information). */
export function containFit(
  dims: { w: number; h: number } | null,
  maxW: number,
  maxH: number,
): { w: number; h: number } {
  if (!dims || dims.w <= 0 || dims.h <= 0 || maxW <= 0 || maxH <= 0) return { w: maxW, h: maxH };
  const scale = Math.min(maxW / dims.w, maxH / dims.h);
  return { w: dims.w * scale, h: dims.h * scale };
}

export interface MediaCounts {
  photos: number;
  videos: number;
}

/** Which member of a pair speaks for the whole group when settling it.
 *  "latest" picks the most recently classified member. */
export type SyncFrom = "raw" | "jpeg" | "latest" | "none";

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
export type DeletionMode = "permanent" | "trash";

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
  /** Per-section digests for the hold-to-run buttons; each validates only its
   *  own section, so committing one leaves the others' hashes valid. */
  deletesHash: string;
  movesHash: string;
  copiesHash: string;
  xmpHash: string;
}

/** A single commit section, targetable by the per-section hold-to-run buttons. */
export type CommitSection = "deletes" | "moves" | "copies" | "xmp";

export interface CommitOutcome {
  commitId: number;
  ok: number;
  errors: number;
  errorSamples: string[];
}

export type SortKey = "capture" | "name" | "size";
export type MediaTab = "photos" | "videos";

export interface ScanProgress {
  found: number;
}
export interface ScanDone {
  fileCount: number;
  newFiles: number;
  missingFiles: number;
  /** Photos whose rating/flag/label were read out of an XMP sidecar this pass. */
  xmpImported: number;
}

export interface Targets {
  ids: number[];
  asGroups: boolean;
}

/** One past commit, as the history list shows it. */
export interface CommitSummary {
  id: number;
  startedAt: number;
  finishedAt: number | null;
  /** 0 = running, 1 = done, 2 = finished with errors. */
  status: number;
  deletes: number;
  moves: number;
  copies: number;
  xmp: number;
  errors: number;
  /** How many of its entries could still be reversed. */
  undoable: number;
  undoneAt: number | null;
}

/** One file's fate inside a commit. */
export interface CommitEntry {
  id: number;
  fileId: number | null;
  /** 0 = delete, 1 = move, 2 = copy, 3 = write XMP. */
  action: number;
  beforePath: string | null;
  afterPath: string | null;
  /** 0 = ok, 2 = error. */
  result: number;
  error: string | null;
  undoneAt: number | null;
  undoable: boolean;
  /** Why it cannot be undone. Shown as-is — the backend owns the wording. */
  blockedReason: string | null;
}

export interface UndoOutcome {
  restored: number;
  skipped: number;
  errors: number;
  errorSamples: string[];
}

export interface CullState {
  id: number;
  rating: number;
  flag: number;
  label: string | null;
  orientation: number;
}

export const api = {
  openProject: (path: string) => invoke<ProjectInfo>("open_project", { path }),
  // Android SAF folder picker; returns a content:// tree URI (or null if
  // cancelled) suitable to pass to openProject. No-op returning null on desktop.
  pickSafTree: () => invoke<string | null>("pick_saf_tree"),
  currentProject: () => invoke<ProjectInfo | null>("current_project"),
  // Files still awaiting metadata (Phase A). Used to recover the open gate if a
  // `metadata:done` event was missed on the startup auto-open path.
  ingestPending: () => invoke<number>("ingest_pending"),
  closeProject: () => invoke("close_project"),
  rescanProject: () => invoke("rescan_project"),
  // Push the "pregenerate video thumbnails" preference to the backend; the
  // ingest pass reads it before its final (slow) video-poster tier.
  setGenerateVideoThumbs: (on: boolean) => invoke("set_generate_video_thumbs", { on }),
  // Push the loupe-preview long edge. Non-destructive and idempotent, so the
  // startup push is free; returns the value actually in force.
  setPreviewQuality: (longEdge: number) => invoke<number>("set_preview_quality", { longEdge }),
  // Delete every generated loupe preview for the open project (cached JPEGs and
  // their rows). Only called after the user confirms a quality change; the
  // rescan that follows regenerates them. Never touches a user file.
  discardPreviews: () => invoke<number>("discard_previews"),
  listRecentProjects: () => invoke<RecentProject[]>("list_recent_projects"),
  // Probe the storage backing a project id (used to watch the open project's
  // folder/volume for disconnection while working).
  probeStorage: (id: string) => invoke<StorageInfo>("probe_storage", { id }),
  // Forget a project AND delete Cullant's own data (DB + thumb cache). The
  // user's photos are never touched.
  deleteProjectData: (path: string) => invoke("delete_project_data", { path }),
  queryItems: (sort: SortKey, media: MediaTab, desc: boolean) =>
    invoke<ItemLite[]>("query_items", { sort, media, desc }),
  mediaCounts: () => invoke<MediaCounts>("media_counts"),
  xmpDirtyCount: () => invoke<number>("xmp_dirty_count"),
  // File ids that already have a generated loupe preview (drives the grid's
  // per-cell "preview still generating" spinner).
  previewReadyIds: () => invoke<number[]>("preview_ready_ids"),
  setRating: (targets: Targets, rating: number) =>
    invoke<CullState[]>("set_rating", { targets, rating }),
  setFlag: (targets: Targets, flag: number) =>
    invoke<CullState[]>("set_flag", { targets, flag }),
  setLabel: (targets: Targets, label: string | null) =>
    invoke<CullState[]>("set_label", { targets, label }),
  /** Turn the targets a quarter turn at a time; `steps` is positive clockwise. */
  rotate: (targets: Targets, steps: number) =>
    invoke<CullState[]>("rotate", { targets, steps }),
  listCommits: () => invoke<CommitSummary[]>("list_commits"),
  commitDetail: (commitId: number) => invoke<CommitEntry[]>("commit_detail", { commitId }),
  undoCommit: (commitId: number) => invoke<UndoOutcome>("undo_commit", { commitId }),
  undoCommitEntry: (entryId: number) => invoke<UndoOutcome>("undo_commit_entry", { entryId }),
  decoupleGroup: (groupId: number) => invoke("decouple_group", { groupId }),
  // Settle a pair whose halves have diverged, without decoupling it.
  syncPairState: (groupId: number, syncFrom: SyncFrom) =>
    invoke<CullState[]>("sync_pair_state", { groupId, syncFrom }),
  recoupleGroup: (groupId: number, syncFrom: SyncFrom) =>
    invoke("recouple_group", { groupId, syncFrom }),
  listTaskTags: () => invoke<TaskTag[]>("list_task_tags"),
  createTaskTag: (name: string, shortcut: string | null, scope: number, color: string | null) =>
    invoke<TaskTag>("create_task_tag", { name, shortcut, scope, color }),
  updateTaskTag: (tag: TaskTag) => invoke("update_task_tag", { tag }),
  deleteTaskTag: (tagId: number) => invoke("delete_task_tag", { tagId }),
  toggleTaskTag: (targets: Targets, tagId: number) =>
    invoke<TagChange[]>("toggle_task_tag", { targets, tagId }),
  clearTaskTags: (targets: Targets) => invoke<TagChange[]>("clear_task_tags", { targets }),
  enqueueAction: (
    targets: Targets,
    action: ActionKind,
    dest: string | null,
    pairScope: PairScope
  ) => invoke<number>("enqueue_action", { targets, action, dest, pairScope }),
  removePending: (pendingIds: number[]) => invoke("remove_pending", { pendingIds }),
  // Remove every pending action of a kind that touches the given files. Used to
  // unqueue deletes when a reject flag is cleared (the mirror of enqueueAction).
  removePendingForFiles: (targets: Targets, action: ActionKind) =>
    invoke<number>("remove_pending_for_files", { targets, action }),
  clearPending: () => invoke("clear_pending"),
  listPending: () => invoke<PendingAction[]>("list_pending"),
  commitPreview: () => invoke<CommitPlan>("commit_preview"),
  commitExecute: (planHash: string) => invoke<CommitOutcome>("commit_execute", { planHash }),
  commitExecuteSection: (section: CommitSection, sectionHash: string) =>
    invoke<CommitOutcome>("commit_execute_section", { section, sectionHash }),
  getProjectSetting: (key: string) => invoke<string | null>("get_project_setting", { key }),
  setProjectSetting: (key: string, value: string) =>
    invoke("set_project_setting", { key, value }),
  // Per-project UI session state (opaque JSON blob; shape owned by the frontend).
  getSessionState: () => invoke<string | null>("get_session_state"),
  setSessionState: (value: string) => invoke("set_session_state", { value }),
  getFileMetadata: (fileId: number) =>
    invoke<FileMetadata>("get_file_metadata", { fileId }),
  // Open a media file in the OS default external app (the "play in an external
  // player" fallback for videos the in-app WebView can't decode).
  openExternal: (fileId: number) => invoke("open_external", { fileId }),
  // Show a file in the OS file manager. Local-filesystem projects only — a SAF
  // project's content:// URI cannot be handed to one.
  revealInExplorer: (fileId: number) => invoke("reveal_in_explorer", { fileId }),
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

/** Cache-buster for the rendered artifacts: mtime plus orientation, because
 *  rotating changes the pixels the backend renders without touching the file.
 *  Must match `CacheVersion` in the backend — the protocol builds its cache
 *  path straight from these params, so a mismatch means a permanent miss. */
export function mediaVersion(item: ItemLite): string {
  return `v=${item.mtime}&o=${item.orientation ?? 1}`;
}

export function thumbUrl(item: ItemLite): string {
  return cullantUrl(`thumb/${item.id}?${mediaVersion(item)}`);
}

export function previewUrl(item: ItemLite): string {
  return cullantUrl(`preview/${item.id}?${mediaVersion(item)}`);
}

export function videoUrl(item: ItemLite): string {
  return cullantUrl(`video/${item.id}?v=${item.mtime}`);
}

/** Preview thumbnail for a homepage recent-project card (0-based list index +
 *  preview slot). `cacheKey` (the project's stable id/path) is appended so the
 *  browser doesn't reuse a cached image when the recent list reorders and a
 *  given index now points at a different project. */
export function recentThumbUrl(index: number, slot: number, cacheKey?: string): string {
  const base = cullantUrl(`recent-thumb/${index}/${slot}`);
  return cacheKey ? `${base}?v=${encodeURIComponent(cacheKey)}` : base;
}
