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
   * A GROUP sector opens into a fan of its members past the rim. The fan shows
   * near the threshold, so a quick flick across the ring stays quiet, and past
   * the threshold it is pinned: the finger turns towards a member and lets go,
   * all in the same motion. The ring stays where it is, so nothing the eye was
   * tracking moves. A fan that fits nowhere on screen falls back to replacing
   * the ring with the group's members.
   *
   * Climbing back out asks for the middle of the hub, deeper than the radius
   * that merely disarms, and a short dwell there — leaving a group is a
   * decision, and it used to happen by accident while crossing the hub to the
   * far side of a fan. Releasing ON a group without opening it opens it as a
   * list rather than doing nothing, so the gesture is never a dead end.
   */
  import { settings } from "../stores/settings.svelte";
  import { tags } from "../stores/tags.svelte";
  import {
    FAN_GAP,
    FAN_MARGIN,
    FAN_MAX,
    FAN_R,
    RING_DESCEND,
    fanAngles,
    sectorAngle,
    slotFace,
    slotShortLabel,
    type RadialSlot,
    type SlotFace,
  } from "../radial";
  import { LABELS, LABEL_COLORS } from "../labels";
  import { formatColorLabel } from "../colorLabels";
  import type { CommandId } from "../keyboard/keymap";
  import RadialRing, { type RingFan, type RingItem } from "./RadialRing.svelte";
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
  /** A replaced ring is left quickly; a fan is often reached THROUGH the hub,
   *  and on a touch screen the finger slows right there as it turns. */
  const CLIMB_DWELL_MS = 160;
  const CLIMB_DWELL_FAN_MS = 300;

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

  /** Root sector whose fan is pinned open, or null. */
  let pinned = $state<number | null>(null);

  const fanGroup = $derived(pinned ?? (pulling ? armed : null));

  const fan = $derived.by(() => {
    if (fanGroup === null) return null;
    const group = rootActions[fanGroup];
    const members = group?.children ?? [];
    if (members.length === 0) return null;
    // Shown only near the threshold, so a quick flick across the ring stays
    // quiet; fully there by the time it pins.
    const t = pinned !== null ? 1 : Math.max(0, Math.min(1, (pull - 0.55) / 0.4));
    if (t <= 0) return null;

    const overflow = members.length > FAN_MAX ? members.length - (FAN_MAX - 1) : 0;
    const shown = overflow ? members.slice(0, FAN_MAX - 1) : members;
    const count = shown.length + (overflow ? 1 : 0);
    const W = window.innerWidth;
    const H = window.innerHeight;
    const fits = (a: number) => {
      const fx = x + FAN_R * Math.cos(a);
      const fy = y + FAN_R * Math.sin(a);
      return fx >= FAN_MARGIN && fx <= W - FAN_MARGIN && fy >= FAN_MARGIN && fy <= H - FAN_MARGIN;
    };
    const centre = sectorAngle(fanGroup, rootActions.length, rot);
    const angles = fanAngles(count, FAN_R, centre, FAN_GAP, fits);
    return angles ? { group, shown, overflow, angles, t } : null;
  });

  /**
   * Fan member the finger points at, or -1. By angle from the ring's centre,
   * the same rule as the ring itself: the finger never has to travel all the
   * way out to a member, only turn towards it.
   */
  const fanArmed = $derived.by(() => {
    if (pinned === null || !fan || dist <= DEAD_ZONE) return -1;
    const a = Math.atan2(dy, dx);
    const half = fan.angles.length > 1 ? Math.abs(fan.angles[1] - fan.angles[0]) / 2 : 0.35;
    let best = -1;
    let bestD = Infinity;
    fan.angles.forEach((fa, j) => {
      const d = Math.abs(Math.atan2(Math.sin(a - fa), Math.cos(a - fa)));
      if (d < bestD) {
        bestD = d;
        best = j;
      }
    });
    // Past either end of the fan points at nothing, so a release there cancels.
    return bestD <= half * 1.8 ? best : -1;
  });

  $effect(() => {
    if (pinned !== null || descended !== null || !pulling) return;
    if (dist <= RING_DESCEND) return;
    // A fan that fits nowhere on screen falls back to replacing the ring with
    // the members, so the group is still reachable. An empty group has
    // nothing to open into.
    if (fan) pinned = armed;
    else if (pulling.children?.length) descended = pulling;
  });

  // Returning through the hub is common while changing direction. Require a
  // short, deliberate dwell before climbing out; leaving the hub cancels the
  // pending climb immediately.
  $effect(() => {
    if ((descended === null && pinned === null) || dist > CLIMB_R) return;
    const current = descended;
    const currentPin = pinned;
    const timer = setTimeout(
      () => {
        if (descended !== current || pinned !== currentPin || dist > CLIMB_R) return;
        descended = null;
        pinned = null;
      },
      pinned !== null ? CLIMB_DWELL_FAN_MS : CLIMB_DWELL_MS,
    );
    return () => clearTimeout(timer);
  });

  $effect(() => {
    if (!released) return;
    if (pinned !== null) {
      if (fan && fanArmed >= 0) {
        // The "+N" place opens the whole group as a list.
        onpick(fanArmed < fan.shown.length ? fan.shown[fanArmed] : fan.group);
      } else oncancel();
      return;
    }
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

  function ringItem(a: RadialAction): RingItem {
    return { label: a.label, icon: a.icon, stars: a.stars, swatch: a.swatch };
  }

  const ringFan = $derived<RingFan | null>(
    fan && {
      angles: fan.angles,
      t: fan.t,
      items: fan.shown.map(ringItem),
      overflow: fan.overflow,
      armed: fanArmed,
    },
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
        label: r === 0 ? "No stars" : r === 1 ? "1 star" : `${r} stars`,
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
    armed={pinned ?? armed}
    {rot}
    pull={pinned !== null ? 1 : pulling ? pull : 0}
    {aim}
    climbable={descended !== null || pinned !== null}
    climbArmed={(descended !== null || pinned !== null) && dist <= CLIMB_R}
    level={descended?.label ?? ""}
    fan={ringFan}
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
