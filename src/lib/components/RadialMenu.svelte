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
    arcPath,
    sectorPoint,
    slotFace,
    slotShortLabel,
    wedgePath,
    type RadialSlot,
  } from "../radial";
  import { LABELS, LABEL_COLORS } from "../labels";
  import type { CommandId } from "../keyboard/keymap";
  import Star from "@lucide/svelte/icons/star";
  import Circle from "@lucide/svelte/icons/circle";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";

  export interface RadialAction {
    label: string;
    /** Drawn in the sector. A `glyph` wins when both are present. */
    icon?: typeof Star;
    /** Short text drawn instead of an icon (star counts read better as stars). */
    glyph?: string;
    /** A colour swatch instead of an icon (labels, tags). */
    swatch?: string;
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
  /** Past this a group sector opens into its members. Deliberately well inside
   *  the drawn ring: having to travel past the edge of what you can see is not
   *  something anyone discovers. */
  const DESCEND_R = 76;
  /** And well inside the dead zone to climb back OUT of one. Sharing the cancel
   *  radius made a group fall open and shut again on the way past, since the
   *  same few pixels meant both "nothing armed" and "leave this group". */
  const CLIMB_R = 15;
  const CLIMB_DWELL_MS = 160;
  const R_INNER = 40;
  const R_OUTER = 108;
  /** The box leaves room for the armed sector to grow past R_OUTER. */
  const BOX = 250;
  const ICON_R = (R_INNER + R_OUTER) / 2;
  /** The armed sector's icon travels out with the wedge that grew under it, so
   *  the sector reads as one moving thing rather than a shape and a label that
   *  happen to share an angle. */
  const ICON_R_ARMED = ICON_R + 9;

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
    Math.max(0, Math.min(1, (dist - DEAD_ZONE) / Math.max(1, DESCEND_R - DEAD_ZONE))),
  );

  // Descending happens on the way out, so it is part of the same motion rather
  // than a second gesture.
  $effect(() => {
    if (descended === null) {
      const hit = armed >= 0 ? rootActions[armed] : undefined;
      if (hit?.children && dist > DESCEND_R) descended = hit;
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
        label: r === 0 ? "None" : `${r}`,
        glyph: r === 0 ? "—" : "★".repeat(r),
        run: () => handlers.rate(r),
      }));
    }
    if (id === "labels") {
      return [
        { label: "None", icon: Circle, run: () => handlers.label(null) },
        ...LABELS.map((l) => ({
          label: l,
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
  <svg class="wedges" width={BOX} height={BOX} viewBox="0 0 {BOX} {BOX}">
    {#each actions as _, i (i)}
      <path
        class="sector"
        class:armed={armed === i}
        d={wedgePath(i, actions.length, R_INNER, R_OUTER, BOX, rot)}
      />
    {/each}

    {#if descended}
      <!-- The way out, drawn where it is: without a mark the hub is a hole, and
           the only clue that going back in closes the group was knowing. -->
      <circle
        class="climb"
        class:armed={dist <= CLIMB_R}
        cx={BOX / 2}
        cy={BOX / 2}
        r={CLIMB_R + 6}
      />
      <path
        class="climb-x"
        class:armed={dist <= CLIMB_R}
        d="M {BOX / 2 - 5} {BOX / 2 - 5} L {BOX / 2 + 5} {BOX / 2 + 5}
           M {BOX / 2 + 5} {BOX / 2 - 5} L {BOX / 2 - 5} {BOX / 2 + 5}"
      />
    {/if}

    {#if pulling}
      <!-- The line the finger has to cross to open this group. It brightens and
           thickens as the pull gets there, so the extra travel a group asks for
           is visible while it is being made instead of only once it works. -->
      <path
        class="gate"
        d={arcPath(armed, actions.length, DESCEND_R, BOX, rot)}
        style="opacity:{0.22 + pull * 0.78}; stroke-width:{1.5 + pull * 3}"
      />
    {/if}
  </svg>

  {#each actions as action, i (i)}
    {@const on = armed === i}
    {@const p = sectorPoint(i, actions.length, on ? ICON_R_ARMED : ICON_R, BOX, rot)}
    <span class="ico" class:armed={on} style="left:{p.x}px; top:{p.y}px">
      {#if action.glyph}
        <span class="glyph">{action.glyph}</span>
      {:else if action.swatch}
        <span class="swatch" style="background:{action.swatch}"></span>
      {:else if action.icon}
        {@const Icon = action.icon}
        <Icon size={19} strokeWidth={2} />
      {/if}
      {#if action.children}
        <span class="submenu-mark" class:armed={on} title="Pull outward to open">
          <ChevronRight size={10} strokeWidth={3} />
        </span>
      {/if}
      {#if on && !action.glyph}
        <!-- Named only while armed, and directly under its own icon so the eye
             never has to travel to find out what it is about to do. A glyph
             sector says it already: "★★★" with "3 stars" under it is the same
             word twice. -->
        <span class="name">{action.label}{action.children ? " · pull out" : ""}</span>
      {/if}
    </span>
  {/each}
</div>

<style>
  .radial {
    position: fixed;
    z-index: 70;
    width: 250px;
    height: 250px;
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .wedges {
    position: absolute;
    inset: 0;
  }

  /* No outline: the gaps between wedges already separate them, and a stroke on
     every sector turns the ring into a diagram. */
  .sector {
    fill: color-mix(in srgb, var(--surface) 88%, transparent);
    /* Not an outline: a stroke in the background colour is the gap between the
       wedges, so they separate without the ring turning into a diagram. */
    stroke: var(--bg-stage);
    stroke-width: 2;
    /* Grown from the ring's centre, so the armed wedge reaches outwards under
       the finger instead of merely changing colour. Short enough to feel like
       feedback rather than an animation. */
    transform-box: view-box;
    transform-origin: 50% 50%;
    transition:
      transform 90ms ease-out,
      fill 90ms ease-out;
  }

  .sector.armed {
    fill: var(--accent-fill);
    transform: scale(1.09);
  }

  .ico {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    transform: translate(-50%, -50%);
    color: #cfd4d4;
    transition:
      color 90ms ease-out,
      left 90ms ease-out,
      top 90ms ease-out,
      transform 90ms ease-out;
  }

  .ico.armed {
    color: #fff;
    transform: translate(-50%, -50%) scale(1.12);
  }

  .submenu-mark {
    position: absolute;
    left: calc(100% + 1px);
    top: 2px;
    display: inline-flex;
    color: var(--accent);
    opacity: 0.8;
    transition:
      opacity 90ms ease-out,
      transform 90ms ease-out;
  }

  .submenu-mark.armed {
    opacity: 1;
    transform: translateX(3px);
  }

  .glyph {
    font-size: 13px;
    font-weight: 700;
    letter-spacing: -1px;
    white-space: nowrap;
  }

  .swatch {
    width: 15px;
    height: 15px;
    border-radius: 50%;
  }

  /* Under the icon, and only for the armed sector. No plate behind it: the
     sector it sits on is already the background, and a pill floating over the
     ring read as a tooltip that had landed in the wrong place. Absolutely
     placed so adding it never nudges the icon it belongs to. */
  .name {
    position: absolute;
    top: 100%;
    margin-top: 2px;
    max-width: 84px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #fff;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.01em;
    white-space: nowrap;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.85);
  }

  .gate {
    fill: none;
    stroke: var(--accent);
    stroke-linecap: round;
  }

  .climb {
    fill: rgba(18, 22, 23, 0.85);
    stroke: var(--border-strong);
    stroke-width: 1;
    transition:
      fill 90ms ease-out,
      stroke 90ms ease-out;
  }

  .climb.armed {
    fill: var(--accent-fill);
    stroke: var(--accent);
  }

  .climb-x {
    fill: none;
    stroke: #9aa0a0;
    stroke-width: 2;
    stroke-linecap: round;
    transition: stroke 90ms ease-out;
  }

  .climb-x.armed {
    stroke: #fff;
  }

</style>
