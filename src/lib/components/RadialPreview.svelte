<script lang="ts">
  /**
   * The ring as the settings panel shows it: the same wedge geometry and the
   * same icons as the live menu, at whatever size it is given.
   *
   * Sharing `wedgePath`/`sectorPoint`/`slotIcon` with `RadialMenu` is the point.
   * A preview drawn from its own copy of the layout would be free to disagree
   * with the thing it claims to be previewing.
   */
  import { sectorPoint, slotFace, slotLabel, wedgePath, type RadialSlot } from "../radial";
  import { settings } from "../stores/settings.svelte";

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
  const rot = $derived((settings.radialRotation * Math.PI) / 180);
</script>

<div class="preview" style="width:{size}px; height:{size}px">
  <svg viewBox="0 0 {BOX} {BOX}" width={size} height={size}>
    {#each slots as slot, i (i)}
      <path
        class="sector"
        class:on={highlight === i}
        d={wedgePath(i, slots.length, R_INNER, R_OUTER, BOX, rot)}
        role="presentation"
        onpointerenter={() => onhighlight?.(i)}
      >
        <title>{slotLabel(slot)}</title>
      </path>
    {/each}
  </svg>

  {#each slots as slot, i (i)}
    {@const p = sectorPoint(i, slots.length, ICON_R, BOX, rot)}
    {@const face = slotFace(slot)}
    <span
      class="ico"
      class:on={highlight === i}
      style="left:{p.x * scale}px; top:{p.y * scale}px"
    >
      {#if face.glyph}
        <span class="glyph">{face.glyph}</span>
      {:else if face.swatch}
        <span class="swatch" style="background:{face.swatch}"></span>
      {:else if face.icon}
        {@const Icon = face.icon}
        <Icon size={16} strokeWidth={2} />
      {/if}
    </span>
  {/each}
</div>

<style>
  .preview {
    position: relative;
    flex: none;
  }

  /* Same fill and same hairline gap as the live ring: a preview drawn to its
     own taste is not a preview. */
  .sector {
    fill: var(--control);
    stroke: var(--surface-2);
    stroke-width: 2;
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

  .glyph {
    font-size: 10px;
    font-weight: 700;
    letter-spacing: -1px;
    white-space: nowrap;
  }

  .swatch {
    width: 13px;
    height: 13px;
    border-radius: 50%;
  }
</style>
