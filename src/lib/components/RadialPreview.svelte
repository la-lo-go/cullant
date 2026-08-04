<script lang="ts">
  /**
   * The ring as the settings panel shows it: the same wedge geometry and the
   * same icons as the live menu, at whatever size it is given.
   *
   * Sharing `wedgePath`/`sectorPoint`/`slotIcon` with `RadialMenu` is the point.
   * A preview drawn from its own copy of the layout would be free to disagree
   * with the thing it claims to be previewing.
   */
  import { sectorPoint, slotIcon, slotLabel, wedgePath, type RadialSlot } from "../radial";

  interface Props {
    slots: RadialSlot[];
    /** Sector to draw as armed, or -1. Lets the panel show which row is which. */
    highlight?: number;
    size?: number;
    onhighlight?: (index: number) => void;
  }

  const { slots, highlight = -1, size = 190, onhighlight }: Props = $props();

  const BOX = 200;
  const R_INNER = 34;
  const R_OUTER = 92;
  const ICON_R = (R_INNER + R_OUTER) / 2;
  const scale = $derived(size / BOX);
</script>

<div class="preview" style="width:{size}px; height:{size}px">
  <svg viewBox="0 0 {BOX} {BOX}" width={size} height={size}>
    {#each slots as slot, i (i)}
      <path
        class="sector"
        class:on={highlight === i}
        d={wedgePath(i, slots.length, R_INNER, R_OUTER, BOX)}
        role="presentation"
        onpointerenter={() => onhighlight?.(i)}
      >
        <title>{slotLabel(slot)}</title>
      </path>
    {/each}
  </svg>

  {#each slots as slot, i (i)}
    {@const p = sectorPoint(i, slots.length, ICON_R, BOX)}
    {@const Icon = slotIcon(slot)}
    <span
      class="ico"
      class:on={highlight === i}
      style="left:{p.x * scale}px; top:{p.y * scale}px"
    >
      <Icon size={16} strokeWidth={2} />
    </span>
  {/each}
</div>

<style>
  .preview {
    position: relative;
    flex: none;
  }

  .sector {
    fill: var(--control);
    stroke: none;
    transform-box: view-box;
    transform-origin: 50% 50%;
    transition:
      transform 90ms ease-out,
      fill 90ms ease-out;
  }

  .sector.on {
    fill: var(--accent-fill);
    transform: scale(1.07);
  }

  .ico {
    position: absolute;
    display: flex;
    align-items: center;
    justify-content: center;
    transform: translate(-50%, -50%);
    color: #b9c0c0;
    pointer-events: none;
    transition: color 90ms ease-out;
  }

  .ico.on {
    color: #fff;
  }
</style>
