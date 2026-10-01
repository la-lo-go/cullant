/**
 * The grid's context menu, as a `MenuNode` tree.
 *
 * Two rules keep this file honest:
 *
 * - Classification goes through `runCommand`, never through `session` directly,
 *   so a right-click and its keyboard shortcut cannot drift apart. Those
 *   commands act on `session.targets()`, which the caller has already pointed at
 *   the right photos before opening the menu.
 * - "Filter by this" only ever sets an axis that already exists. Filters are
 *   per-axis, so picking a camera leaves the rating filter alone, and an axis
 *   that is already set offers to clear itself instead.
 */

import { api, type ItemLite, type SyncFrom } from "./api";
import { dayKey, hasGroupValue, usefulGroupDims } from "./gridGroups";
import { runCommand } from "./keyboard/dispatcher.svelte";
import type { CommandId } from "./keyboard/keymap";
import { pruneMenu, type MenuNode } from "./menu";
import { HAS_FILE_MANAGER } from "./platform";
import { catalog } from "./stores/catalog.svelte";
import {
  apertureBucket,
  focalBucket,
  isoBucket,
  shutterBucket,
  type FacetBucket,
} from "./metadataFacets";
import { LABELS, session } from "./stores/session.svelte";
import { tags } from "./stores/tags.svelte";
import { formatColorLabel } from "./colorLabels";
import { view } from "./stores/view.svelte";

import ArrowDownUp from "@lucide/svelte/icons/arrow-down-up";
import Check from "@lucide/svelte/icons/check";
import Copy from "@lucide/svelte/icons/copy";
import Eraser from "@lucide/svelte/icons/eraser";
import ExternalLink from "@lucide/svelte/icons/external-link";
import Filter from "@lucide/svelte/icons/filter";
import FolderOpen from "@lucide/svelte/icons/folder-open";
import Group from "@lucide/svelte/icons/group";
import Info from "@lucide/svelte/icons/info";
import Layers from "@lucide/svelte/icons/layers";
import Link2 from "@lucide/svelte/icons/link-2";
import LayoutGrid from "@lucide/svelte/icons/layout-grid";
import Maximize2 from "@lucide/svelte/icons/maximize-2";
import SquareDashedMousePointer from "@lucide/svelte/icons/square-dashed-mouse-pointer";
import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
import RotateCw from "@lucide/svelte/icons/rotate-cw";
import Scissors from "@lucide/svelte/icons/scissors";
import SlidersHorizontal from "@lucide/svelte/icons/sliders-horizontal";
import Star from "@lucide/svelte/icons/star";
import Tag from "@lucide/svelte/icons/tag";
import Trash2 from "@lucide/svelte/icons/trash-2";
import X from "@lucide/svelte/icons/x";

/** Rating commands by star count, so the submenu indexes them instead of
 *  building a `CommandId` out of string pieces. */
const RATE_COMMANDS: CommandId[] = ["rate.0", "rate.1", "rate.2", "rate.3", "rate.4", "rate.5"];

/**
 * One filter axis as a menu row: applies the clicked photo's value, or clears
 * the axis when that value is already the active one. `active` is the axis's
 * current value so the row can also show as checked when a *different* value is
 * set — the user still sees the axis is in play.
 */
function axisRow(
  label: string,
  value: string | null | undefined,
  current: string | null,
  set: (v: string | null) => void,
): MenuNode[] {
  if (value == null || value === "") return [];
  const isThis = current === value;
  return [
    {
      kind: "item",
      label: isThis ? `Clear ${label.toLowerCase()} filter` : `${label}: ${value}`,
      checked: isThis,
      run: () => {
        set(isThis ? null : value);
        session.clampFocus();
      },
    },
  ];
}

/** Same, for the numeric settings that filter by bucket key but read as a label. */
function bucketRow(
  label: string,
  bucket: FacetBucket | null,
  current: string | null,
  set: (v: string | null) => void,
): MenuNode[] {
  if (!bucket) return [];
  const isThis = current === bucket.key;
  return [
    {
      kind: "item",
      label: isThis ? `Clear ${label.toLowerCase()} filter` : `${label}: ${bucket.label}`,
      checked: isThis,
      run: () => {
        set(isThis ? null : bucket.key);
        session.clampFocus();
      },
    },
  ];
}

function classifyBlock(item: ItemLite): MenuNode[] {
  const isPair = item.groupSize > 1;
  const scopedTags = tags.all.filter(
    (t) => t.scope === 2 || (catalog.media === "photos" ? t.scope === 0 : t.scope === 1),
  );

  return [
    {
      kind: "item",
      label: "Pick",
      icon: Check,
      hint: "P",
      checked: item.flag === 1,
      run: () => runCommand("flag.pick"),
    },
    {
      kind: "item",
      label: "Reject",
      icon: X,
      hint: "X",
      checked: item.flag === -1,
      run: () => runCommand("flag.reject"),
    },
    {
      kind: "item",
      label: "Unflag",
      hint: "U",
      checked: item.flag === 0,
      run: () => runCommand("flag.unflag"),
    },
    {
      kind: "submenu",
      label: "Rating",
      icon: Star,
      children: [0, 1, 2, 3, 4, 5].map((r) => ({
        kind: "item" as const,
        label: r === 0 ? "None" : "★".repeat(r),
        hint: `${r}`,
        checked: item.rating === r,
        run: () => runCommand(RATE_COMMANDS[r]),
      })),
    },
    {
      kind: "submenu",
      label: "Color label",
      children: [
        {
          kind: "item",
          label: "None",
          checked: item.label == null,
          run: () => void session.setLabel(null),
        },
        ...LABELS.map((l) => ({
          kind: "item" as const,
          label: formatColorLabel(l),
          checked: item.label === l,
          run: () => void session.setLabel(item.label === l ? null : l),
        })),
      ],
    },
    {
      kind: "submenu",
      label: "Tags",
      icon: Tag,
      children: scopedTags.map((t) => ({
        kind: "item" as const,
        label: t.name,
        checked: item.tagIds.includes(t.id),
        run: () => void session.toggleTag(t.id),
      })),
    },
    {
      kind: "item",
      label: "Clear classification",
      icon: Eraser,
      run: () => void session.clearClassification(),
    },
    { kind: "sep" },
    {
      kind: "item",
      label: "Rotate left",
      icon: RotateCcw,
      run: () => runCommand("edit.rotateLeft"),
    },
    {
      kind: "item",
      label: "Rotate right",
      icon: RotateCw,
      run: () => runCommand("edit.rotateRight"),
    },
    { kind: "sep" },
    {
      kind: "item",
      label: "Queue delete",
      icon: Trash2,
      hint: "Del",
      run: () => runCommand("delete.pair"),
    },
    ...(isPair
      ? ([
          {
            kind: "item",
            label: "Queue delete: RAW only",
            hint: "Alt+Del",
            run: () => runCommand("delete.rawOnly"),
          },
          {
            kind: "item",
            label: "Queue delete: JPEG only",
            hint: "Shift+Del",
            run: () => runCommand("delete.jpegOnly"),
          },
        ] as MenuNode[])
      : []),
    {
      kind: "item",
      label: "Move or copy…",
      icon: FolderOpen,
      run: () => runCommand("action.moveCopy"),
    },
    ...(isPair
      ? ([
          { kind: "sep" },
          // Only offered once the halves actually disagree: settling a pair that
          // already agrees is a no-op dressed up as a choice.
          ...(!item.decoupled && session.pairHalves(item)?.diverged
            ? [
                {
                  kind: "submenu" as const,
                  label: "Settle pair from…",
                  icon: Link2,
                  children: (
                    [
                      ["raw", "RAW"],
                      ["jpeg", "JPEG"],
                      ["latest", "Most recent edit"],
                    ] as [SyncFrom, string][]
                  ).map(([from, label]) => ({
                    kind: "item" as const,
                    label,
                    run: () => void session.syncPair(item.groupId, from),
                  })),
                },
              ]
            : []),
          {
            kind: "item",
            label: item.decoupled ? "Recouple pair…" : "Decouple pair",
            icon: Scissors,
            hint: "Ctrl+J",
            run: () => runCommand("pair.toggleCoupling"),
          },
          {
            kind: "item",
            label: "Show the other half",
            hint: "J",
            run: () => runCommand("pair.toggleShown"),
          },
        ] as MenuNode[])
      : []),
  ];
}

function filterBlock(item: ItemLite): MenuNode[] {
  const photos = catalog.media === "photos";

  const filters: MenuNode[] = [
    ...(photos
      ? [
          ...axisRow("Camera", item.camera, session.cameraFilter, (v) => (session.cameraFilter = v)),
          ...axisRow("Lens", item.lens, session.lensFilter, (v) => (session.lensFilter = v)),
          ...bucketRow("ISO", isoBucket(item.iso), session.isoFilter, (v) => (session.isoFilter = v)),
          ...bucketRow(
            "Aperture",
            apertureBucket(item.fNumber),
            session.apertureFilter,
            (v) => (session.apertureFilter = v),
          ),
          ...bucketRow(
            "Focal length",
            focalBucket(item.focalLength),
            session.focalFilter,
            (v) => (session.focalFilter = v),
          ),
          ...bucketRow(
            "Shutter",
            shutterBucket(item.exposureTime),
            session.shutterFilter,
            (v) => (session.shutterFilter = v),
          ),
        ]
      : []),
  ];

  return [
    ...(filters.length > 0
      ? [
          {
            kind: "submenu" as const,
            label: "Filter by this…",
            icon: Filter,
            children: filters,
          },
        ]
      : []),
    {
      kind: "submenu",
      label: "Group by this…",
      icon: Group,
      children: usefulGroupDims(session.filtered, session.groupContext, session.groupBy)
        .filter((d) => hasGroupValue(d, item, session.groupContext))
        .map((d) => ({
          kind: "item" as const,
          label: d.label,
          checked: session.groupBy.length === 1 && session.groupBy[0] === d.key,
          run: () =>
            (session.groupBy =
              session.groupBy.length === 1 && session.groupBy[0] === d.key ? [] : [d.key]),
        })),
    },
    {
      kind: "submenu",
      label: "Sort by",
      icon: ArrowDownUp,
      children: (["capture", "name", "size"] as const).map((key) => ({
        kind: "item" as const,
        label: { capture: "Capture time", name: "File name", size: "File size" }[key],
        checked: catalog.sort === key,
        run: () => void catalog.setSort(key),
      })),
    },
  ];
}

/** Every file id in `filtered` matching a predicate, for the "select similar"
 *  rows. Runs over the visible list on purpose: selecting photos hidden by the
 *  current filter would act on things the user cannot see. */
function idsWhere(match: (i: ItemLite) => boolean): number[] {
  return session.filtered.filter(match).map((i) => i.id);
}

function selectBlock(index: number, item: ItemLite): MenuNode[] {
  const burstKey = session.bursts.byFile.get(item.id) ?? null;
  const burstRun = burstKey === null ? undefined : session.burstRuns.get(burstKey);
  const isFocused = session.focused?.id === item.id;

  const similar: MenuNode[] = [
    ...(item.camera
      ? [
          {
            kind: "item" as const,
            label: `Same camera (${item.camera})`,
            run: () => session.selectIds(idsWhere((i) => i.camera === item.camera)),
          },
        ]
      : []),
    ...(item.label
      ? [
          {
            kind: "item" as const,
            label: `Same color label (${item.label})`,
            run: () => session.selectIds(idsWhere((i) => i.label === item.label)),
          },
        ]
      : []),
    {
      kind: "item",
      label: item.rating === 0 ? "Same rating (none)" : `Same rating (${"★".repeat(item.rating)})`,
      run: () => session.selectIds(idsWhere((i) => i.rating === item.rating)),
    },
    {
      kind: "item",
      label: `Same day (${dayKey(item)})`,
      run: () => session.selectIds(idsWhere((i) => dayKey(i) === dayKey(item))),
    },
  ];

  return [
    {
      kind: "item",
      label: "Open in loupe",
      icon: Maximize2,
      hint: "Enter",
      run: () => {
        session.selectOnly(index);
        view.markOpenedFromGrid();
        view.mode = "viewer";
      },
    },
    {
      kind: "item",
      label: "Compare with focused",
      icon: LayoutGrid,
      disabled: isFocused || session.focused === undefined,
      run: () => session.compareWith(index),
    },
    ...(burstRun && burstRun.length > 1
      ? ([
          {
            kind: "item",
            label: `Survey this burst (${burstRun.length})`,
            icon: Layers,
            hint: "N",
            run: () => {
              session.selectIds(burstRun.map((i) => session.filtered[i].id));
              session.openSurvey();
            },
          },
          {
            kind: "item",
            label: session.expandedBursts.has(burstKey!) ? "Collapse this burst" : "Expand this burst",
            run: () => session.toggleBurstExpanded(burstKey!),
          },
        ] as MenuNode[])
      : []),
    { kind: "sep" },
    ...(burstRun && burstRun.length > 1
      ? ([
          {
            kind: "item",
            label: `Select this burst (${burstRun.length})`,
            icon: SquareDashedMousePointer,
            run: () => session.selectIds(burstRun.map((i) => session.filtered[i].id)),
          },
        ] as MenuNode[])
      : []),
    { kind: "submenu", label: "Select similar", icon: SquareDashedMousePointer, children: similar },
    {
      kind: "item",
      label: "Select all",
      hint: "Ctrl+A",
      run: () => runCommand("select.all"),
    },
    {
      kind: "item",
      label: "Invert selection",
      hint: "Ctrl+I",
      run: () => runCommand("select.invert"),
    },
  ];
}

function fileBlock(index: number, item: ItemLite): MenuNode[] {
  return [
    ...(!HAS_FILE_MANAGER
      ? []
      : ([
          {
            kind: "item",
            label: "Show in Explorer",
            icon: FolderOpen,
            run: () => void api.revealInExplorer(item.id).catch(() => {}),
          },
        ] as MenuNode[])),
    {
      kind: "item",
      label: "Open with external app",
      icon: ExternalLink,
      run: () => void api.openExternal(item.id).catch(() => {}),
    },
    {
      kind: "item",
      label: "Copy path",
      icon: Copy,
      run: () => void navigator.clipboard?.writeText(item.relPath).catch(() => {}),
    },
    {
      kind: "item",
      label: "Copy file name",
      run: () => void navigator.clipboard?.writeText(`${item.name}.${item.ext}`).catch(() => {}),
    },
    {
      kind: "item",
      label: "Metadata",
      icon: Info,
      hint: "I",
      // The metadata panel lives inside the loupe, so opening it from the grid
      // has to take the photo there too — setting `infoOpen` alone renders
      // nothing and reads as a dead menu entry.
      run: () => {
        session.selectOnly(index);
        view.markOpenedFromGrid();
        view.mode = "viewer";
        view.infoOpen = true;
      },
    },
  ];
}

/** The menu for a right-click on empty grid space: nothing is under the cursor,
 *  so only the view-wide rows apply. */
function emptySpaceMenu(): MenuNode[] {
  return [
    // No "clear selection" row: opening this menu already dropped the selection
    // and the focus, because the click that opened it landed on nothing.
    { kind: "item", label: "Select all", hint: "Ctrl+A", run: () => runCommand("select.all") },
    { kind: "sep" },
    {
      kind: "item",
      label: "Sort & filter…",
      icon: SlidersHorizontal,
      run: () => (session.filtersPanelOpen = true),
    },
    {
      kind: "item",
      label: "Clear all filters",
      icon: Eraser,
      disabled: !session.hasActiveFilters,
      run: () => session.clearFilters(),
    },
    {
      kind: "item",
      label: "View options…",
      icon: LayoutGrid,
      run: () => (session.viewPanelOpen = true),
    },
  ];
}

/** Build the menu for a right-click at `index` (a `filtered` index), or on empty
 *  space when it is null. */
export function buildGridMenu(index: number | null): MenuNode[] {
  if (index === null) return pruneMenu(emptySpaceMenu());
  const item = session.filtered[index];
  if (!item) return pruneMenu(emptySpaceMenu());
  return pruneMenu([
    ...classifyBlock(item),
    { kind: "sep" },
    ...filterBlock(item),
    { kind: "sep" },
    ...selectBlock(index, item),
    { kind: "sep" },
    ...fileBlock(index, item),
  ]);
}
