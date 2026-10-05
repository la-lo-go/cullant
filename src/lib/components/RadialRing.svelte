<script lang="ts" module>
  import type { SlotFace } from "../radial";

  /** One sector as the ring draws it. */
  export interface RingItem extends SlotFace {
    label: string;
    /** Opens a ring of its own when the finger keeps moving outwards. */
    group?: boolean;
  }
</script>

<script lang="ts">
  /**
   * The ring itself: wedges, faces and the group marks, with no gesture
   * logic. The live menu feeds it a pointer; the settings panel feeds
   * it a highlighted row. Both draw THIS, so the preview can never drift from
   * what the finger will actually see.
   *
   * Drawn in a fixed `RING_BOX` square and scaled as a whole, so a smaller
   * preview keeps the exact proportions instead of a hand-tuned copy of them.
   */
  import {
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
    /** Inside a group: the hub is the way back out. */
    climbable?: boolean;
    climbArmed?: boolean;
    /** Changes when the ring is replaced, so the new one animates in. */
    level?: string;
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
   *  needs; the live box keeps room for the armed wedge to grow. */
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
                {#if p > 0}
                  <path
                    class="fill"
                    d={wedgePath(i, n, innerOf(i), RING_INNER + p * (RING_OUTER - RING_INNER), RING_BOX, rot)}
                  />
                {/if}
                <!-- A second rim just outside the first: there is a layer
                     behind this sector. -->
                <path
                  class="group-mark"
                  d={arcPath(i, n, RING_OUTER + 5, RING_BOX, rot, groupMarkSpan)}
                  style="opacity:{0.45 + p * 0.55}"
                />
              {/if}
            </g>
          {/each}

          {#if aim && !preview && !climbable}
            <path class="aim" d={aimPath} style="opacity:{aim.strength}" />
          {/if}

          {#if climbable}
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
    transition: transform 90ms ease-out;
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

  .group-mark {
    fill: none;
    stroke: var(--accent);
    stroke-width: 2.5;
    stroke-linecap: round;
    transition: opacity 120ms ease-out;
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
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.01em;
    white-space: nowrap;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.85);
  }
</style>
