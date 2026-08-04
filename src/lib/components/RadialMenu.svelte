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
   * A GROUP sector descends: keep moving outwards and the whole ring is
   * replaced by that group's members, so their sectors stay as wide as the
   * ones they came from. Coming back to the dead zone climbs out again.
   */
  import { LABELS } from "../stores/session.svelte";
  import { settings } from "../stores/settings.svelte";
  import { tags } from "../stores/tags.svelte";
  import { slotLabel, type RadialSlot } from "../radial";
  import type { CommandId } from "../keyboard/keymap";

  /** One thing the ring can carry out. */
  export interface RadialAction {
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
  /** Past this, a group sector opens into its members. */
  const DESCEND_R = 116;
  /** Drawn ring geometry. */
  const R_INNER = 44;
  const R_OUTER = 112;
  const LABEL_R = (R_INNER + R_OUTER) / 2;

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
    let a = Math.atan2(dx, -dy);
    if (a < 0) a += Math.PI * 2;
    return Math.floor((a + step / 2) / step) % n;
  });

  // Descending happens on the way out, so it is part of the same motion rather
  // than a second gesture. Climbing back out needs a return to the dead zone,
  // which is also how the ring is cancelled — one rule, not two.
  $effect(() => {
    if (descended === null) {
      const hit = armed >= 0 ? rootActions[armed] : undefined;
      if (hit?.children && dist > DESCEND_R) descended = hit;
    } else if (dist <= DEAD_ZONE) {
      descended = null;
    }
  });

  $effect(() => {
    if (!released) return;
    const hit = armed >= 0 ? actions[armed] : undefined;
    if (hit && (hit.run || hit.more)) onpick(hit);
    else oncancel();
  });

  function buildRoot(slots: RadialSlot[]): RadialAction[] {
    return slots.map((slot) => {
      if (slot.kind === "more") return { label: "More…", more: true };
      if (slot.kind === "command") {
        return { label: slotLabel(slot), run: () => handlers.run(slot.id) };
      }
      return { label: slotLabel(slot), children: groupChildren(slot.id) };
    });
  }

  function groupChildren(id: "stars" | "labels" | "tags"): RadialAction[] {
    if (id === "stars") {
      return [0, 1, 2, 3, 4, 5].map((r) => ({
        label: r === 0 ? "None" : "★".repeat(r),
        run: () => handlers.rate(r),
      }));
    }
    if (id === "labels") {
      return [
        { label: "None", run: () => handlers.label(null) },
        ...LABELS.map((l) => ({ label: l, run: () => handlers.label(l) })),
      ];
    }
    return tags.all.map((t) => ({ label: t.name, run: () => handlers.tag(t.id) }));
  }

  /** SVG wedge for sector `i` of `n`, centred on straight up. */
  function wedge(i: number, n: number): string {
    const step = (Math.PI * 2) / n;
    const a0 = i * step - step / 2 - Math.PI / 2;
    const a1 = a0 + step;
    const p = (r: number, a: number) => `${R_OUTER + r * Math.cos(a)} ${R_OUTER + r * Math.sin(a)}`;
    const large = step > Math.PI ? 1 : 0;
    return (
      `M ${p(R_INNER, a0)} A ${R_INNER} ${R_INNER} 0 ${large} 1 ${p(R_INNER, a1)} ` +
      `L ${p(R_OUTER, a1)} A ${R_OUTER} ${R_OUTER} 0 ${large} 0 ${p(R_OUTER, a0)} Z`
    );
  }

  function labelPos(i: number, n: number): { x: number; y: number } {
    const a = i * ((Math.PI * 2) / n) - Math.PI / 2;
    return { x: R_OUTER + LABEL_R * Math.cos(a), y: R_OUTER + LABEL_R * Math.sin(a) };
  }
</script>

<div class="radial" style="left:{x}px; top:{y}px" aria-hidden="true">
  <svg width={R_OUTER * 2} height={R_OUTER * 2} viewBox="0 0 {R_OUTER * 2} {R_OUTER * 2}">
    {#each actions as _, i (i)}
      <path class="sector" class:armed={armed === i} d={wedge(i, actions.length)} />
    {/each}
    {#each actions as action, i (i)}
      {@const p = labelPos(i, actions.length)}
      <text class="label" class:armed={armed === i} x={p.x} y={p.y}>{action.label}</text>
    {/each}
    <circle class="hub" class:armed={armed < 0} cx={R_OUTER} cy={R_OUTER} r={DEAD_ZONE} />
    <text class="hub-label" x={R_OUTER} y={R_OUTER}>
      {descended ? descended.label : armed < 0 ? "Cancel" : ""}
    </text>
  </svg>
</div>

<style>
  .radial {
    position: fixed;
    z-index: 70;
    transform: translate(-50%, -50%);
    pointer-events: none;
  }

  .sector {
    fill: rgba(28, 34, 35, 0.92);
    stroke: var(--border-strong);
    stroke-width: 1;
  }

  .sector.armed {
    fill: var(--accent-fill);
  }

  .label {
    fill: #e8e8e8;
    font-size: 11px;
    font-weight: 600;
    text-anchor: middle;
    dominant-baseline: middle;
    paint-order: stroke;
    stroke: rgba(0, 0, 0, 0.75);
    stroke-width: 3px;
  }

  .label.armed {
    fill: #fff;
  }

  .hub {
    fill: rgba(18, 22, 23, 0.94);
    stroke: var(--border-strong);
    stroke-width: 1;
  }

  .hub.armed {
    stroke: var(--accent);
  }

  .hub-label {
    fill: #9aa0a0;
    font-size: 10px;
    text-anchor: middle;
    dominant-baseline: middle;
  }
</style>
