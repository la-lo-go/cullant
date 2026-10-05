<script lang="ts">
  /**
   * Press-and-hold ring over a photo.
   *
   * Spring-loaded, and only that: the finger is already down when this mounts,
   * so the gesture is one continuous motion — flick to a sector, release, done.
   * Releasing in the dead zone cancels, which is what makes the ring safe to
   * open by accident.
   *
   * Selection is by ANGLE, not by hit-testing an element. A sector 60° wide is
   * forgiving at speed in a way a shape under the finger never is, and the
   * finger is usually somewhere past the ring's edge by the time it stops.
   *
   * Sectors carry an icon and nothing else; the name appears only for the one
   * under the finger. Reading six labels at speed is slower than recognising six
   * shapes, and a sector is not wide enough for "Queue delete: JPEG only".
   *
   * A GROUP sector descends: keep moving outwards and the whole ring is replaced
   * by that group's members, so their sectors stay as wide as the ones they came
   * from. Climbing back out asks for the middle of the hub, deeper than the
   * radius that merely disarms — leaving a group is a decision, and it used to
   * happen by accident while crossing the dead zone between two sectors.
   * Releasing ON a group without descending opens it as a list rather than doing
   * nothing, so the gesture is never a dead end.
   */
  import { settings } from "../stores/settings.svelte";
  import { tags } from "../stores/tags.svelte";
  import {
    RING_DESCEND,
    slotFace,
    slotShortLabel,
    type RadialSlot,
    type SlotFace,
  } from "../radial";
  import { LABELS, LABEL_COLORS } from "../labels";
  import { formatColorLabel } from "../colorLabels";
  import type { CommandId } from "../keyboard/keymap";
  import RadialRing, { type RingItem } from "./RadialRing.svelte";
  import Circle from "@lucide/svelte/icons/circle";
  import StarOff from "@lucide/svelte/icons/star-off";

  export interface RadialAction extends SlotFace {
    label: string;
    /** Present for a leaf; absent on a group, which descends instead. */
    run?: () => void;
    /** Members to descend into. */
    children?: RadialAction[];
    /** Hands over to the full command list rather than acting. */
    more?: boolean;
  }

  /** What the ring's leaves actually do. The parent owns these, because only it
   *  knows which pane was pressed and therefore which photo is the target. */
  export interface RadialHandlers {
    run: (id: CommandId) => void;
    rate: (rating: number) => void;
    label: (label: string | null) => void;
    tag: (tagId: number) => void;
  }

  interface Props {
    /** Where the finger went down, in client coordinates. */
    x: number;
    y: number;
    /** Live pointer position; the parent owns the pointer and feeds it here. */
    px: number;
    py: number;
    /** Set when the pointer is released; the ring acts on what was armed. */
    released: boolean;
    handlers: RadialHandlers;
    onpick: (action: RadialAction) => void;
    oncancel: () => void;
  }

  const { x, y, px, py, released, handlers, onpick, oncancel }: Props = $props();

  /** Inside this radius nothing is armed, so a release cancels. */
  const DEAD_ZONE = 34;
  /** And well inside the dead zone to climb back OUT of a group. Sharing the
   *  cancel radius made a group fall open and shut again on the way past, since
   *  the same few pixels meant both "nothing armed" and "leave this group". */
  const CLIMB_R = 15;
  const CLIMB_DWELL_MS = 160;

  /** The whole ring turned, so the first sector can sit where a given hand
   *  actually reaches rather than always straight up. */
  const rot = $derived((settings.radialRotation * Math.PI) / 180);

  const rootActions = $derived(buildRoot(settings.radialSlots));

  /** Which group the ring has descended into, or null at the top level. */
  let descended = $state<RadialAction | null>(null);

  const actions = $derived(descended?.children ?? rootActions);

  const dx = $derived(px - x);
  const dy = $derived(py - y);
  const dist = $derived(Math.hypot(dx, dy));

  /**
   * Sector under the pointer, or -1 inside the dead zone. Sector 0 is centred on
   * straight up and they run clockwise, so the angle is measured from the top.
   */
  const armed = $derived.by(() => {
    if (dist <= DEAD_ZONE || actions.length === 0) return -1;
    const n = actions.length;
    const step = (Math.PI * 2) / n;
    // atan2(dx, -dy) is 0 straight up and grows clockwise.
    let a = Math.atan2(dx, -dy) - rot;
    a %= Math.PI * 2;
    if (a < 0) a += Math.PI * 2;
    return Math.floor((a + step / 2) / step) % n;
  });

  /** The armed sector, when it is a group still waiting to be opened. */
  const pulling = $derived.by(() => {
    if (descended !== null || armed < 0) return null;
    const hit = rootActions[armed];
    return hit?.children ? hit : null;
  });

  /** How far towards opening that group the finger has travelled, 0 to 1. The
   *  ring shows this: a group that needs a firmer pull than a plain command
   *  should say so while the finger is still on the way. */
  const pull = $derived(
    Math.max(0, Math.min(1, (dist - DEAD_ZONE) / Math.max(1, RING_DESCEND - DEAD_ZONE))),
  );

  // Descending happens on the way out, so it is part of the same motion rather
  // than a second gesture. An empty group has nothing to descend into.
  $effect(() => {
    if (descended === null && pulling?.children?.length && dist > RING_DESCEND) {
      descended = pulling;
    }
  });

  // Returning through the hub is common while changing direction inside a
  // submenu. Require a short, deliberate dwell before climbing out; leaving
  // the hub cancels the pending climb immediately.
  $effect(() => {
    if (descended === null || dist > CLIMB_R) return;
    const current = descended;
    const timer = setTimeout(() => {
      if (descended === current && dist <= CLIMB_R) descended = null;
    }, CLIMB_DWELL_MS);
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!released) return;
    const hit = armed >= 0 ? actions[armed] : undefined;
    if (hit && (hit.run || hit.more || hit.children)) onpick(hit);
    else oncancel();
  });

  const ringItems = $derived<RingItem[]>(
    actions.map((a) => ({
      label: a.label,
      icon: a.icon,
      stars: a.stars,
      swatch: a.swatch,
      group: a.children !== undefined,
    })),
  );

  /** Which way the finger is heading, for the hub's direction cue. */
  const aim = $derived(
    dist > 6 ? { angle: Math.atan2(dx, -dy), strength: Math.min(1, (dist - 6) / 28) } : null,
  );

  function buildRoot(slots: RadialSlot[]): RadialAction[] {
    return slots.map((slot) => {
      const base = { label: slotShortLabel(slot), ...slotFace(slot) };
      if (slot.kind === "more") return { ...base, more: true };
      if (slot.kind === "command") return { ...base, run: () => handlers.run(slot.id) };
      return { ...base, children: groupChildren(slot.id) };
    });
  }

  function groupChildren(id: "stars" | "labels" | "tags"): RadialAction[] {
    if (id === "stars") {
      return [0, 1, 2, 3, 4, 5].map((r) => ({
        label: r === 0 ? "No stars" : `${r}`,
        ...(r === 0 ? { icon: StarOff } : { stars: r }),
        run: () => handlers.rate(r),
      }));
    }
    if (id === "labels") {
      return [
        { label: "None", icon: Circle, run: () => handlers.label(null) },
        ...LABELS.map((l) => ({
          label: formatColorLabel(l),
          swatch: LABEL_COLORS[l],
          run: () => handlers.label(l),
        })),
      ];
    }
    return tags.all
      .filter((t) => t.scope === 2 || t.scope === 0)
      .map((t) => ({
        label: t.name,
        swatch: t.color ?? "#888",
        run: () => handlers.tag(t.id),
      }));
  }

</script>

<div class="radial" style="left:{x}px; top:{y}px" aria-hidden="true">
  <RadialRing
    items={ringItems}
    {armed}
    {rot}
    pull={pulling ? pull : 0}
    {aim}
    climbable={descended !== null}
    climbArmed={descended !== null && dist <= CLIMB_R}
    level={descended?.label ?? ""}
  />
</div>

<style>
  .radial {
    position: fixed;
    z-index: 70;
    transform: translate(-50%, -50%);
    pointer-events: none;
  }
</style>
