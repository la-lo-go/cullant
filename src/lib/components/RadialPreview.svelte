<script lang="ts">
  /**
   * The ring as the settings panel shows it: the live menu's own `RadialRing`,
   * scaled down. A preview drawn from its own copy of the layout would be free
   * to disagree with the thing it claims to be previewing.
   */
  import { slotFace, slotShortLabel, type RadialSlot } from "../radial";
  import { settings } from "../stores/settings.svelte";
  import RadialRing, { type RingItem } from "./RadialRing.svelte";

  interface Props {
    slots: RadialSlot[];
    /** Sector to draw as armed, or -1. Lets the panel show which row is which. */
    highlight?: number;
    size?: number;
    onhighlight?: (index: number) => void;
  }

  const { slots, highlight = -1, size = 190, onhighlight }: Props = $props();

  const rot = $derived((settings.radialRotation * Math.PI) / 180);

  const items = $derived<RingItem[]>(
    slots.map((slot) => ({
      label: slotShortLabel(slot),
      ...slotFace(slot),
      group: slot.kind === "group",
    })),
  );
</script>

<RadialRing {items} armed={highlight} {rot} {size} preview {onhighlight} />
