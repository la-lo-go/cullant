/**
 * Commands aimed at ONE photo rather than at the current focus or selection.
 *
 * The radial menu opens over a pane, and in compare that pane need not be the
 * focused one — so `runCommand`, which acts on `session.targets()`, would hit
 * the wrong photo. Everything here takes the targets explicitly instead, which
 * is why the session's classification methods grew an override.
 *
 * Anything with no per-photo meaning (navigation, view switching, opening the
 * commit dialog) falls through to `runCommand` unchanged.
 */

import type { ItemLite, Targets } from "./api";
import { runCommand } from "./keyboard/dispatcher.svelte";
import { COMMANDS, type CommandId } from "./keyboard/keymap";
import { pruneMenu, type MenuItemNode, type MenuNode } from "./menu";
import { LABELS, session } from "./stores/session.svelte";
import { tags } from "./stores/tags.svelte";

const RATINGS: Record<string, number> = {
  "rate.0": 0,
  "rate.1": 1,
  "rate.2": 2,
  "rate.3": 3,
  "rate.4": 4,
  "rate.5": 5,
};

const LABEL_OF: Record<string, string> = {
  "label.red": "Red",
  "label.yellow": "Yellow",
  "label.green": "Green",
  "label.blue": "Blue",
  "label.purple": "Purple",
};

/** Run `id` against `targets` instead of the current focus/selection. */
export function runPhotoCommand(id: CommandId, item: ItemLite, targets: Targets) {
  if (id in RATINGS) return void session.rate(RATINGS[id], undefined, targets);
  if (id in LABEL_OF) {
    // Same-label-again clears it, matching the keyboard's `label()`.
    const next = LABEL_OF[id];
    return void session.setLabel(item.label === next ? null : next, targets);
  }
  switch (id) {
    case "flag.pick":
      return void session.flag(1, undefined, targets);
    case "flag.reject":
      return void session.flag(-1, undefined, targets);
    case "flag.unflag":
      return void session.flag(0, undefined, targets);
    case "flag.toggle":
      return void session.flag(item.flag === 1 ? 0 : 1, undefined, targets);
    case "delete.pair":
      return void session.queueDelete("both", undefined, targets);
    case "delete.rawOnly":
      return void session.queueDelete("rawonly", undefined, targets);
    case "delete.jpegOnly":
      return void session.queueDelete("jpegonly", undefined, targets);
    case "edit.rotateLeft":
      return void session.rotate(-1, targets);
    case "edit.rotateRight":
      return void session.rotate(1, targets);
    default:
      // No per-photo reading: let it mean what it always means.
      return runCommand(id);
  }
}

/** Every command this photo can be given, for the ring's "More…" list. Built
 *  from the same registry the ring assigns from, so a command can never be
 *  offered in one and missing from the other. */
export function buildPhotoMenu(item: ItemLite, targets: Targets): MenuNode[] {
  const isPair = item.groupSize > 1;
  const scopedTags = tags.all.filter((t) => t.scope === 2 || t.scope === (item.kind === 2 ? 1 : 0));
  const cmd = (id: CommandId, extra?: Partial<MenuItemNode>): MenuNode => ({
    kind: "item",
    label: COMMANDS.find((c) => c.id === id)?.title ?? id,
    run: () => runPhotoCommand(id, item, targets),
    ...extra,
  });

  return pruneMenu([
    { kind: "header", label: "Flags" },
    cmd("flag.pick", { checked: item.flag === 1 }),
    cmd("flag.reject", { checked: item.flag === -1 }),
    cmd("flag.unflag", { checked: item.flag === 0 }),
    {
      kind: "submenu",
      label: "Rating",
      children: [0, 1, 2, 3, 4, 5].map((r) => ({
        kind: "item" as const,
        label: r === 0 ? "None" : "★".repeat(r),
        checked: item.rating === r,
        run: () => void session.rate(r, undefined, targets),
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
          run: () => void session.setLabel(null, targets),
        },
        ...LABELS.map((l) => ({
          kind: "item" as const,
          label: l,
          checked: item.label === l,
          run: () => void session.setLabel(item.label === l ? null : l, targets),
        })),
      ],
    },
    {
      kind: "submenu",
      label: "Tags",
      children: scopedTags.map((t) => ({
        kind: "item" as const,
        label: t.name,
        checked: item.tagIds.includes(t.id),
        run: () => void session.toggleTag(t.id, undefined, targets),
      })),
    },
    { kind: "sep" },
    cmd("edit.rotateLeft"),
    cmd("edit.rotateRight"),
    { kind: "sep" },
    cmd("delete.pair"),
    ...(isPair ? [cmd("delete.rawOnly"), cmd("delete.jpegOnly")] : []),
    ...(isPair
      ? ([
          { kind: "sep" },
          ...(!item.decoupled && session.pairHalves(item)?.diverged
            ? [
                {
                  kind: "submenu" as const,
                  label: "Settle pair from…",
                  children: (
                    [
                      ["raw", "RAW"],
                      ["jpeg", "JPEG"],
                      ["latest", "Most recent edit"],
                    ] as const
                  ).map(([from, label]) => ({
                    kind: "item" as const,
                    label,
                    run: () => void session.syncPair(item.groupId, from),
                  })),
                },
              ]
            : []),
          cmd("pair.toggleShown"),
          cmd("pair.toggleCoupling"),
        ] as MenuNode[])
      : []),
  ]);
}
