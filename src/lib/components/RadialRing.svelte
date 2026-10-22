<script lang="ts" module>
  import type { SlotFace } from "../radial";

  /** One sector as the ring draws it. */
  export interface RingItem extends SlotFace {
    label: string;
    /** Opens into a fan of its members when the finger keeps moving outwards. */
    group?: boolean;
  }

  /** A group's members fanned out past the rim. */
  export interface RingFan {
    /** SVG angle of each place, the "+N" place last when there is one. */
    angles: number[];
    /** 0 to 1: how far it has come in. */
    t: number;
    items: RingItem[];
    /** Members left out; drawn as a "+N" place. */
    overflow: number;
    /** Place the finger points at, or -1. */
    armed: number;
  }
</script>

<script lang="ts">
  /**
   * The ring itself: wedges, faces, the group marks and the fan, with no
   * gesture logic. The live menu feeds it a pointer; the settings panel feeds
   * it a highlighted row. Both draw THIS, so the preview can never drift from
   * what the finger will actually see.
   *
   * Drawn in a fixed `RING_BOX` square and scaled as a whole, so a smaller
   * preview keeps the exact proportions instead of a hand-tuned copy of them.
   */
  import {
    FAN_CHIP,
    FAN_R,
    RING_BOX,
    RING_ICON,
    RING_INNER,
    RING_OUTER,
    arcPath,
    sectorPoint,
    wedgePath,
  } from "../radial";
  import Star from "@lucide/svelte/icons/star";

  interface Props {
    items: RingItem[];
    /** Sector under the finger (or the highlighted row), or -1. */
    armed?: number;
    /** Whole-ring rotation, in radians. */
    rot?: number;
    /** Rendered width in px. Defaults to the live size. */
    size?: number;
    /** The settings diagram: drawn exactly like the live ring, only smaller,
     *  still and without the gesture's own feedback. */
    preview?: boolean;
    /** 0 to 1: how far the finger has pulled towards opening the armed group. */
    pull?: number;
    /** Pointer direction, clockwise from straight up, and how sure it is (0–1). */
    aim?: { angle: number; strength: number } | null;
    /** The hub is the way back out of an open group. */
    climbable?: boolean;
    climbArmed?: boolean;
    /** Changes when the ring is replaced, so the new one animates in. */
    level?: string;
    fan?: RingFan | null;
    onhighlight?: (index: number) => void;
  }

  const {
    items,
    armed = -1,
    rot = 0,
    size = RING_BOX,
    preview = false,
    pull = 0,
    aim = null,
    climbable = false,
    climbArmed = false,
    level = "",
    fan = null,
    onhighlight,
  }: Props = $props();

  const C = RING_BOX / 2;
  /** Flush with the wedges' inner edge, whose stroke covers the seam: any gap
   *  showed the photo through as a hairline ring of its own. */
  const HUB_R = RING_INNER;
  const CLIMB_R = 21;
  const ICON_R_ARMED = RING_ICON + 7;
  /** Group marks keep to this arc length, whatever the sector's width. Scaled
   *  with the sector, the mark of a two-sector ring was half the ring's
   *  outline. */
  const GROUP_MARK_LEN = 56;

  /** The armed wedge grows by this much. */
  const GROW = 1.09;
  const n = $derived(items.length);
  const step = $derived((Math.PI * 2) / Math.max(1, n));

  /** The preview crops to the ring plus the room a highlighted group mark
   *  needs; the live box keeps room for the fan as well. */
  const crop = $derived(preview ? 2 * (RING_OUTER + 18) : RING_BOX);
  const k = $derived(size / crop);
  const offset = $derived(((crop - RING_BOX) / 2) * k);

  /** The armed wedge is scaled about the centre, so the growth can ease in on
   *  every webview. Its inner edge is drawn that much smaller to stay put:
   *  growing inwards pushed it into the hub. */
  function innerOf(i: number): number {
    return armed === i ? RING_INNER / GROW : RING_INNER;
  }

  /** Pull applies only to the armed sector, and only when it is a group. */
  function pullOf(i: number): number {
    return !preview && armed === i && items[i]?.group ? pull : 0;
  }

  const groupMarkSpan = $derived.by(() => {
    const inset = Math.min(step * 0.12, 0.12);
    const length = (step - inset * 2) * (RING_OUTER + 5);
    return Math.min(0.7, GROUP_MARK_LEN / length);
  });

  function polar(a: number, r: number): { x: number; y: number } {
    return { x: C + r * Math.cos(a), y: C + r * Math.sin(a) };
  }

  /** The hub's direction cue: a short arc on its edge, facing the finger. */
  const aimPath = $derived.by(() => {
    if (!aim) return "";
    const half = 0.5;
    const a = aim.angle - Math.PI / 2;
    const r = HUB_R - 6;
    const p0 = polar(a - half, r);
    const p1 = polar(a + half, r);
    return `M ${p0.x} ${p0.y} A ${r} ${r} 0 0 1 ${p1.x} ${p1.y}`;
  });

  const fanName = $derived.by(() => {
    if (!fan || fan.armed < 0) return null;
    const member = fan.items[fan.armed];
    return member ? member.label : `${fan.overflow} more…`;
  });

  /** Slides out from the rim as it fades in, rather than popping into place. */
  const fanPoints = $derived(
    fan ? fan.angles.map((a) => polar(a, FAN_R - (1 - fan.t) * 14)) : [],
  );

  /** A pinned fan is where the choice happens now; the ring is only the way
   *  back, and the group it came from only needs to stay recognisable. */
  const pinned = $derived(fan !== null && fan.t >= 1 && climbable);

  function starRows(count: number): number[] {
    if (count <= 3) return [count];
    return count === 4 ? [2, 2] : [3, 2];
  }
</script>

{#snippet face(f: SlotFace, iconSize: number)}
  {#if f.stars !== undefined}
    <span class="stars">
      {#each starRows(f.stars) as row, r (r)}
        <span class="star-row">
          {#each { length: row } as _, s (s)}
            <Star size={iconSize * 0.55} fill="currentColor" strokeWidth={1.5} />
          {/each}
        </span>
      {/each}
    </span>
  {:else if f.swatch}
    <span class="swatch" style="background:{f.swatch}; width:{iconSize * 0.8}px; height:{iconSize * 0.8}px"></span>
  {:else if f.icon}
    {@const Icon = f.icon}
    <Icon size={iconSize} strokeWidth={2} />
  {/if}
{/snippet}

<div class="ring" style="width:{size}px; height:{size}px">
  <div
    class="box"
    class:pinned
    style="width:{RING_BOX}px; height:{RING_BOX}px; left:{offset}px; top:{offset}px; transform:scale({k})"
  >
    {#key level}
      <div class="layer" class:enter={!preview}>
        <svg width={RING_BOX} height={RING_BOX} viewBox="0 0 {RING_BOX} {RING_BOX}">
          <circle class="hub" cx={C} cy={C} r={HUB_R} />

          {#each items as item, i (i)}
            {@const on = armed === i}
            {@const p = pullOf(i)}
            <g class="slice" class:armed={on} style="--grow:{GROW}">
              <path
                class="sector"
                d={wedgePath(i, n, innerOf(i), RING_OUTER, RING_BOX, rot)}
                role="presentation"
                onpointerenter={() => onhighlight?.(i)}
              >
                {#if preview}<title>{item.label}</title>{/if}
              </path>

              {#if item.group}
                {#if p > 0 && !pinned}
                  <path
                    class="fill"
                    d={wedgePath(i, n, innerOf(i), RING_INNER + p * (RING_OUTER - RING_INNER), RING_BOX, rot)}
                  />
                {/if}
                <!-- A second rim just outside the first: there is a layer
                     behind this sector. As the fan comes in it drifts out
                     towards it and fades, so the layer reads as having BECOME
                     the fan rather than vanishing from under it. -->
                {@const handover = on && fan ? fan.t : 0}
                <path
                  class="group-mark"
                  d={arcPath(i, n, RING_OUTER + 5, RING_BOX, rot, groupMarkSpan)}
                  style="opacity:{(0.45 + p * 0.55) * (1 - handover)}; transform:scale({1 + handover * 0.14})"
                />
              {/if}
            </g>
          {/each}

          {#if aim && !preview && !climbable}
            <path class="aim" d={aimPath} style="opacity:{aim.strength}" />
          {/if}

          {#if climbable && !fanName}
            <!-- The way out, drawn where it is: without a mark the hub is a hole,
                 and the only clue that going back in closes the group was
                 knowing. -->
            <circle class="climb" class:armed={climbArmed} cx={C} cy={C} r={CLIMB_R} />
            <path
              class="climb-x"
              class:armed={climbArmed}
              d="M {C - 5} {C - 5} L {C + 5} {C + 5} M {C + 5} {C - 5} L {C - 5} {C + 5}"
            />
          {/if}
        </svg>

        {#each items as item, i (i)}
          {@const on = armed === i}
          {@const pt = sectorPoint(i, n, on ? ICON_R_ARMED : RING_ICON, RING_BOX, rot)}
          <span
            class="ico"
            class:armed={on}
            class:dim={armed >= 0 && !on}
            style="left:{pt.x}px; top:{pt.y}px"
          >
            {@render face(item, 19)}
            {#if on && item.stars === undefined}
              <!-- Named only while armed, and directly under its own face so the
                   eye never has to travel to find out what it is about to do. A
                   star count needs no caption. -->
              <span class="name">{item.label}</span>
            {/if}
          </span>
        {/each}

        {#if fanName}
          <!-- In the hub, not beside the chip: beside it the name covered the
               next member or ran off the edge, and the hub is the one place
               that is always on screen, since the finger went down there. -->
          <span class="hub-name">{fanName}</span>
        {/if}

        {#if fan}
          {#each fanPoints as q, j (j)}
            {@const member = fan.items[j]}
            <span
              class="chip"
              class:armed={fan.armed === j}
              style="left:{q.x}px; top:{q.y}px; width:{FAN_CHIP}px; height:{FAN_CHIP}px; opacity:{fan.t}; --s:{0.7 + fan.t * 0.3}"
            >
              {#if member}
                {@render face(member, 15)}
              {:else}
                <span class="more">+{fan.overflow}</span>
              {/if}
            </span>
          {/each}
        {/if}
      </div>
    {/key}
  </div>
</div>

<style>
  .ring {
    position: relative;
    flex: none;
  }

  .box {
    position: absolute;
    transform-origin: 0 0;
  }

  .layer {
    position: absolute;
    inset: 0;
  }

  /* Each ring arrives rather than appears. Short: the gesture is already
     moving. */
  .layer.enter {
    animation: ring-in 110ms ease-out;
  }

  @keyframes ring-in {
    from {
      opacity: 0;
      transform: scale(0.88);
    }
  }

  svg {
    position: absolute;
    inset: 0;
    overflow: visible;
  }

  /* Lifted off the photo: a ring over a bright frame otherwise reads as part
     of the picture. */
  svg {
    filter: drop-shadow(0 6px 18px rgba(0, 0, 0, 0.5));
  }

  .hub {
    fill: color-mix(in srgb, var(--bg-stage) 92%, transparent);
  }

  .aim {
    fill: none;
    stroke: var(--accent);
    stroke-width: 3;
    stroke-linecap: round;
  }

  /* Grown from the ring's centre, so the armed wedge reaches outwards under the
     finger instead of merely changing colour. */
  .slice {
    transform-box: view-box;
    transform-origin: 50% 50%;
    transition:
      transform 90ms ease-out,
      opacity 120ms ease-out;
  }

  .slice.armed {
    transform: scale(var(--grow));
  }

  /* No outline: a stroke in the background colour is the gap between the
     wedges, so they separate without the ring turning into a diagram. */
  .sector {
    fill: color-mix(in srgb, var(--surface) 92%, transparent);
    stroke: var(--bg-stage);
    stroke-width: 2;
    transition: fill 120ms ease-out;
  }

  .slice.armed .sector {
    fill: var(--accent-fill);
  }

  .box.pinned .slice.armed .sector {
    fill: color-mix(in srgb, var(--accent-fill) 55%, var(--surface));
  }

  /* With a fan open, everything but the group it came from steps back. */
  .box.pinned .slice:not(.armed) {
    opacity: 0.45;
  }

  .group-mark {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2.5;
    stroke-linecap: round;
    transform-box: view-box;
    transform-origin: 50% 50%;
    /* Smooths the steps between pointer events, and eases the mark back in
       when the finger climbs out of the fan. */
    transition:
      opacity 120ms ease-out,
      transform 120ms ease-out;
  }

  .fill {
    fill: var(--accent);
    opacity: 0.35;
    pointer-events: none;
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

  .ico {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: center;
    transform: translate(-50%, -50%);
    color: #cfd4d4;
    pointer-events: none;
    transition:
      color 90ms ease-out,
      opacity 90ms ease-out,
      left 90ms ease-out,
      top 90ms ease-out,
      transform 90ms ease-out;
  }

  .ico.armed {
    color: #fff;
    transform: translate(-50%, -50%) scale(1.12);
  }

  /* The rest step back while one sector is armed, so the eye lands on it. */
  .ico.dim {
    opacity: 0.6;
  }

  .box.pinned .ico.dim {
    opacity: 0.3;
  }

  .stars {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1px;
  }

  .star-row {
    display: flex;
    gap: 1px;
  }

  .swatch {
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25);
  }

  /* Under the face, and only for the armed sector. No plate behind it: the
     sector is already the background. Absolutely placed so adding it never
     nudges the face it belongs to. */
  .name {
    position: absolute;
    top: 100%;
    margin-top: 3px;
    max-width: 84px;
    overflow: hidden;
    text-overflow: ellipsis;
    color: #fff;
    font-size: var(--fs-xs);
    font-weight: 600;
    letter-spacing: 0.01em;
    white-space: nowrap;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.85);
  }

  .hub-name {
    position: absolute;
    left: 50%;
    top: 50%;
    max-width: 66px;
    overflow: hidden;
    transform: translate(-50%, -50%);
    color: #fff;
    font-size: 11px;
    font-weight: 600;
    text-align: center;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }

  /* Every member on the same round plate, so equal steps are equal gaps. A
     plate behind it at all because the fan sits past the ring, on the photo
     itself, and stars on a bright frame vanish without one. */
  .chip {
    position: absolute;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    background: color-mix(in srgb, var(--surface) 94%, transparent);
    color: #e6eaea;
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.45);
    pointer-events: none;
    transform: translate(-50%, -50%) scale(var(--s));
    transition:
      background 90ms ease-out,
      color 90ms ease-out;
  }

  /* Colour alone marks it; raised so a neighbour can never sit on top. */
  .chip.armed {
    z-index: 1;
    background: var(--accent-fill);
    color: #fff;
  }

  .more {
    font-size: 11px;
    font-weight: 700;
  }
</style>
