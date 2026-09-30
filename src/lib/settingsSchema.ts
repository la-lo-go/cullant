/**
 * The settings dialog, as data.
 *
 * Every preference is declared once here and the dialog derives everything from
 * it: the two-column desktop layout, the single-column mobile one, the text
 * behind each row's info affordance, the search index, the "changed from
 * default" dot and the reset buttons. Adding a preference is one entry, and it
 * cannot then be missing from the search or the reset while being present in the
 * list.
 *
 * The four rows that are not a plain toggle or select declare a `kind` the
 * dialog renders itself: two open a sub-panel, and preview quality has to
 * confirm before it applies because it invalidates every generated preview.
 */

import { BOTTOM_BAR_ITEMS, DEFAULTS, settings } from "./stores/settings.svelte";
import { DEFAULT_RADIAL_SLOTS, slotKey } from "./radial";
import type { BurstMode } from "./bursts";
import { AUTO_RESCAN_CHOICES, BURST_GAP_CHOICES } from "./stores/settings.svelte";
import Zap from "@lucide/svelte/icons/zap";
import Eye from "@lucide/svelte/icons/eye";
import Gauge from "@lucide/svelte/icons/gauge";
import Layers from "@lucide/svelte/icons/layers";
import FolderOpen from "@lucide/svelte/icons/folder-open";
import Film from "@lucide/svelte/icons/film";
import PanelBottom from "@lucide/svelte/icons/panel-bottom";
import CircleDot from "@lucide/svelte/icons/circle-dot";

export type GroupId = "culling" | "appearance" | "quality" | "bursts" | "project";

export type SubPanelId = "filmstripBadges" | "touchBar" | "radial";

export type CustomSlot = "previewQuality";

export interface SettingGroup {
  id: GroupId;
  label: string;
  icon: typeof Zap;
}

/** Declaration order is display order, and it also decides which column each
 *  group lands in on desktop: the dialog splits the list in two so the columns
 *  come out close to the same height. */
export const GROUPS: SettingGroup[] = [
  { id: "culling", label: "Culling", icon: Zap },
  { id: "appearance", label: "Appearance", icon: Eye },
  { id: "quality", label: "Quality & performance", icon: Gauge },
  { id: "bursts", label: "Bursts", icon: Layers },
  { id: "project", label: "Project & files", icon: FolderOpen },
];

interface SettingBase {
  id: string;
  group: GroupId;
  label: string;
  /** The text behind the info affordance. The search field indexes it, so a word
   *  the user is likely to type has to appear here and not only in the label. */
  info: string;
  /** Search terms present in neither the label nor the info, for the cases where
   *  the user's word for a thing is not the one the UI uses. */
  keywords?: string;
  modified(): boolean;
  reset(): void;
}

export interface ToggleSetting extends SettingBase {
  kind: "toggle";
  get(): boolean;
  set(v: boolean): void;
}

export interface ChoiceSetting extends SettingBase {
  kind: "choice";
  options: { value: string; label: string }[];
  /** Stringified, because that is what a <select> deals in. Each declaration
   *  parses it back to whatever the store wants. */
  get(): string;
  set(v: string): void;
}

export interface PanelSetting extends SettingBase {
  kind: "panel";
  panel: SubPanelId;
  /** Drawn on the door, so it reads as one from across the dialog. */
  icon: typeof Zap;
}

export interface CustomSetting extends SettingBase {
  kind: "custom";
  slot: CustomSlot;
}

export type Setting = ToggleSetting | ChoiceSetting | PanelSetting | CustomSetting;

function toggle(s: {
  id: string;
  group: GroupId;
  label: string;
  info: string;
  keywords?: string;
  get: () => boolean;
  set: (v: boolean) => void;
  def: boolean;
}): ToggleSetting {
  return {
    kind: "toggle",
    id: s.id,
    group: s.group,
    label: s.label,
    info: s.info,
    keywords: s.keywords,
    get: s.get,
    set: s.set,
    modified: () => s.get() !== s.def,
    reset: () => s.set(s.def),
  };
}

function numberChoice(s: {
  id: string;
  group: GroupId;
  label: string;
  info: string;
  keywords?: string;
  get: () => number;
  set: (v: number) => void;
  def: number;
  options: { value: number; label: string }[];
}): ChoiceSetting {
  return {
    kind: "choice",
    id: s.id,
    group: s.group,
    label: s.label,
    info: s.info,
    keywords: s.keywords,
    options: s.options.map((o) => ({ value: String(o.value), label: o.label })),
    get: () => String(s.get()),
    set: (v) => s.set(Number(v)),
    modified: () => s.get() !== s.def,
    reset: () => s.set(s.def),
  };
}

/** The five filmstrip badge toggles, as one addressable unit: the sub-panel
 *  renders them and the parent row summarises and resets them together. */
export const FILMSTRIP_BADGES: { label: string; get(): boolean; set(v: boolean): void }[] = [
  {
    label: "Photo type (RAW+JPG)",
    get: () => settings.filmstripShowType,
    set: (v) => settings.setFilmstripShowType(v),
  },
  {
    label: "Star rating",
    get: () => settings.filmstripShowRating,
    set: (v) => settings.setFilmstripShowRating(v),
  },
  {
    label: "Color label",
    get: () => settings.filmstripShowLabel,
    set: (v) => settings.setFilmstripShowLabel(v),
  },
  {
    label: "Pick / reject flag",
    get: () => settings.filmstripShowFlag,
    set: (v) => settings.setFilmstripShowFlag(v),
  },
  {
    label: "Tags",
    get: () => settings.filmstripShowTags,
    set: (v) => settings.setFilmstripShowTags(v),
  },
];

const BADGE_DEFAULTS = [
  DEFAULTS.filmstripShowType,
  DEFAULTS.filmstripShowRating,
  DEFAULTS.filmstripShowLabel,
  DEFAULTS.filmstripShowFlag,
  DEFAULTS.filmstripShowTags,
];

const DEFAULT_BAR_ORDER = BOTTOM_BAR_ITEMS.map((i) => i.id).join();

function barReordered(): boolean {
  return settings.bottomBarOrder.join() !== DEFAULT_BAR_ORDER;
}

export const SETTINGS: Setting[] = [
  toggle({
    id: "fastCulling",
    group: "culling",
    label: "Fast culling",
    info: "In the loupe and compare views, any rating, flag, label or tag jumps to the next photo. Hold Shift to stay put.",
    keywords: "auto advance next caps lock",
    get: () => settings.fastCulling,
    set: (v) => settings.setFastCulling(v),
    def: DEFAULTS.fastCulling,
  }),
  toggle({
    id: "skipRejected",
    group: "culling",
    label: "Skip photos marked for deletion",
    info: "Next and previous step over them in the loupe and compare views. Picking a thumbnail still opens a marked photo, and so does scrolling one into the centre with the carousel locked.",
    keywords: "reject delete navigate",
    get: () => settings.skipRejected,
    set: (v) => settings.setSkipRejected(v),
    def: DEFAULTS.skipRejected,
  }),
  toggle({
    id: "lockCarousel",
    group: "culling",
    label: "Lock carousel",
    info: "Scrolling the filmstrip moves the loupe to the centred photo, instead of scrolling on its own.",
    keywords: "filmstrip strip scroll",
    get: () => settings.lockCarousel,
    set: (v) => settings.setLockCarousel(v),
    def: DEFAULTS.lockCarousel,
  }),
  toggle({
    id: "compareZoomSync",
    group: "culling",
    label: "Sync zoom in Compare",
    info: "Keeps both panes on the same relative point at the same physical pixel scale. Either pane can lead while you zoom or pan.",
    keywords: "compare linked zoom pan focus pixels",
    get: () => settings.compareZoomSync,
    set: (v) => settings.setCompareZoomSync(v),
    def: DEFAULTS.compareZoomSync,
  }),

  toggle({
    id: "dimQueuedDeletes",
    group: "appearance",
    label: "Dim thumbnails marked for deletion",
    info: "Fades them in the grid and the filmstrip, so a doomed photo reads at a glance. The red X badge stays fully visible.",
    keywords: "reject delete fade grey",
    get: () => settings.dimQueuedDeletes,
    set: (v) => settings.setDimQueuedDeletes(v),
    def: DEFAULTS.dimQueuedDeletes,
  }),
  toggle({
    id: "dimDeletesInPreview",
    group: "appearance",
    label: "Also dim the large photo",
    info: "Extends that fade to the loupe and compare views. Press and hold the photo to drop the fade for as long as you hold it.",
    keywords: "reject delete fade loupe compare preview",
    get: () => settings.dimDeletesInPreview,
    set: (v) => settings.setDimDeletesInPreview(v),
    def: DEFAULTS.dimDeletesInPreview,
  }),
  {
    kind: "panel",
    panel: "filmstripBadges",
    icon: Film,
    id: "filmstripBadges",
    group: "appearance",
    label: "Filmstrip badges",
    info: "Which badges the filmstrip thumbnails carry. Set apart from the grid's, because a filmstrip cell is small enough for a badge to sit outside the photo.",
    keywords: "raw jpg star rating color label flag tags",
    modified: () => FILMSTRIP_BADGES.some((b, i) => b.get() !== BADGE_DEFAULTS[i]),
    reset: () => FILMSTRIP_BADGES.forEach((b, i) => b.set(BADGE_DEFAULTS[i])),
  },
  {
    kind: "panel",
    panel: "touchBar",
    icon: PanelBottom,
    id: "touchBar",
    group: "appearance",
    label: "Action bar",
    info: "Drag the handles to change the order of the action groups. Use the checkboxes to show or hide each group.",
    keywords: "bottom bar reorder hide mobile rating labels tags move copy",
    modified: () => settings.bottomBarHidden.length > 0 || barReordered(),
    reset: () => settings.resetBottomBar(),
  },
  {
    kind: "panel",
    panel: "radial",
    icon: CircleDot,
    id: "radial",
    group: "culling",
    label: "Radial menu",
    info: "Press and hold a photo in the loupe or compare to open the ring.",
    keywords: "hold press gesture wheel pie sector sectors touch thumb mouse button left right either click opening rotation rotate angle degrees",
    modified: () =>
      settings.radialMouse !== "left" ||
      settings.radialSlots.map(slotKey).join() !== DEFAULT_RADIAL_SLOTS.map(slotKey).join(),
    reset: () => settings.resetRadial(),
  },

  {
    kind: "custom",
    slot: "previewQuality",
    id: "previewQuality",
    group: "quality",
    label: "Preview quality",
    info: "How sharp the loupe preview is. Lower is faster to generate and uses far less memory, which is worth it on a phone. Changing this rebuilds every preview.",
    keywords: "sharpness resolution size memory pixels",
    modified: () => settings.previewQuality !== DEFAULTS.previewQuality,
    reset: () => settings.setPreviewQuality(DEFAULTS.previewQuality),
  },
  toggle({
    id: "progressiveLoupe",
    group: "quality",
    label: "Progressive loading",
    info: "Paints the cached thumbnail instantly while the sharp preview decodes, so opening a photo is never a blank frame.",
    keywords: "thumbnail placeholder blur speed",
    get: () => settings.progressiveLoupe,
    set: (v) => settings.setProgressiveLoupe(v),
    def: DEFAULTS.progressiveLoupe,
  }),
  toggle({
    id: "generateVideoThumbs",
    group: "quality",
    label: "Video poster frames",
    info: "Generates one poster frame per video in the background. They always run last, after every photo thumbnail and preview, because extracting them is the slow tier. On desktop this needs ffmpeg on PATH; on Android it uses the system media decoder.",
    keywords: "video movie mp4 mov ffmpeg thumbnail",
    get: () => settings.generateVideoThumbs,
    set: (v) => settings.setGenerateVideoThumbs(v),
    def: DEFAULTS.generateVideoThumbs,
  }),

  {
    kind: "choice",
    id: "burstMode",
    group: "bursts",
    label: "Threshold",
    info: "How the burst threshold is chosen. Adaptive reads the shoot's own rhythm and falls back to the fixed gap when the intervals show no clear split, because a sports shoot and a wedding do not photograph alike.",
    keywords: "adaptive automatic fixed gap",
    options: [
      { value: "fixed", label: "Fixed" },
      { value: "adaptive", label: "Adaptive" },
    ],
    get: () => settings.burstMode,
    set: (v) => settings.setBurstMode(v as BurstMode),
    modified: () => settings.burstMode !== DEFAULTS.burstMode,
    reset: () => settings.setBurstMode(DEFAULTS.burstMode),
  },
  numberChoice({
    id: "burstGapSeconds",
    group: "bursts",
    label: "Fixed gap",
    info: "Shots closer together than this belong to the same burst. A burst never spans two cameras, and a RAW+JPEG pair always stays together.",
    keywords: "seconds interval time",
    get: () => settings.burstGapSeconds,
    set: (v) => settings.setBurstGapSeconds(v),
    def: DEFAULTS.burstGapSeconds,
    options: BURST_GAP_CHOICES.map((s) => ({ value: s, label: `${s} s` })),
  }),

  numberChoice({
    id: "autoRescanMinutes",
    group: "project",
    label: "Auto-rescan",
    info: "How often to check the project folder for files added, removed or changed outside Cullant. Only runs while the storage is reachable, so a disconnected drive is not polled.",
    keywords: "sync refresh watch folder interval minutes off",
    get: () => settings.autoRescanMinutes,
    set: (v) => settings.setAutoRescanMinutes(v),
    def: DEFAULTS.autoRescanMinutes,
    options: AUTO_RESCAN_CHOICES.map((m) => ({
      value: m,
      label: m === 0 ? "Off" : `${m} min`,
    })),
  }),
  toggle({
    id: "rememberSession",
    group: "project",
    label: "Remember session per project",
    info: "Restores each project's last sort, media tab, filters and focused photo when you reopen it. Stored in that project's own database.",
    keywords: "restore resume filters sort",
    get: () => settings.rememberSession,
    set: (v) => settings.setRememberSession(v),
    def: DEFAULTS.rememberSession,
  }),
];

/** Case-insensitive match over label, info and keywords, so searching a concept
 *  ("delete") finds the settings that never say it in their label. */
export function matches(s: Pick<Setting, "label" | "info" | "keywords"> & { group?: GroupId }, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const group = GROUPS.find((g) => g.id === s.group)?.label ?? "";
  const haystack = `${s.label} ${s.info} ${s.keywords ?? ""} ${group}`.toLowerCase();
  return q.split(/\s+/).every((word) => haystack.includes(word));
}

export function settingsOf(group: GroupId): Setting[] {
  return SETTINGS.filter((s) => s.group === group);
}

export function groupModified(group: GroupId): boolean {
  return settingsOf(group).some((s) => s.modified());
}

export function resetGroup(group: GroupId) {
  for (const s of settingsOf(group)) s.reset();
}

export function anyModified(): boolean {
  return SETTINGS.some((s) => s.modified());
}

export function resetAll() {
  for (const s of SETTINGS) s.reset();
}
