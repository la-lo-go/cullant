<script lang="ts">
  // Overlay scrollbar thumb. Chromium removed `overflow: overlay`, so a
  // native classic scrollbar always carves its gutter out of the container's
  // content box (8px lost to layout). Hosts hide the native bar on their
  // scroll container (`scrollbar-width: none` + `::-webkit-scrollbar {
  // display: none }`) and render this component over the content instead, so
  // the bar costs no layout space. Visuals match the app-wide scrollbar
  // styling in +page.svelte (~4px accent pill inset 2px from the edge).
  let {
    orientation,
    viewport,
    content,
    position,
    controls,
    onSeek,
    alwaysVisible = false,
  }: {
    orientation: "vertical" | "horizontal";
    /** Visible size of the scroll container along the scroll axis (px). */
    viewport: number;
    /** Total scrollable content size along the scroll axis (px). */
    content: number;
    /** Current scrollTop/scrollLeft of the container (px). */
    position: number;
    /** id of the scroll container this bar controls (aria-controls). */
    controls: string;
    /** Scroll the container to the given offset (px, may be un-clamped). */
    onSeek: (position: number) => void;
    /** Skip the auto-hide-while-idle behavior below — stays visible the whole
     *  time it's scrollable. The filmstrip/carousel opts into this; every
     *  other host (folder tree) gets the default auto-hide. */
    alwaysVisible?: boolean;
  } = $props();

  const MIN_THUMB = 24;
  const INSET = 2; // gap between the thumb's ends and the container corners
  // How long the thumb lingers after the last scroll before fading out.
  const HIDE_DELAY_MS = 900;

  const track = $derived(Math.max(0, viewport - INSET * 2));
  const scrollable = $derived(content > viewport + 1 && track > 0);
  const thumbSize = $derived(
    Math.min(track, Math.max(MIN_THUMB, (viewport / content) * track))
  );
  const maxScroll = $derived(Math.max(1, content - viewport));
  const fraction = $derived(Math.min(1, Math.max(0, position / maxScroll)));
  const thumbPos = $derived(INSET + fraction * (track - thumbSize));

  let dragging = $state(false);
  let dragPointer = -1;
  let dragStart = 0;
  let dragStartScroll = 0;

  // Show the thumb only while actively scrolling (plus a short linger), fading
  // in/out quickly — a scrollbar sitting on screen at rest reads as clutter.
  // Always-visible hosts (the filmstrip) skip this and just stay shown.
  let idleVisible = $state(false);
  const shown = $derived(alwaysVisible || idleVisible || dragging);
  $effect(() => {
    void position;
    if (alwaysVisible) return;
    idleVisible = true;
    const t = setTimeout(() => {
      idleVisible = false;
    }, HIDE_DELAY_MS);
    return () => clearTimeout(t);
  });

  function coord(e: PointerEvent) {
    return orientation === "vertical" ? e.clientY : e.clientX;
  }

  function onDown(e: PointerEvent) {
    e.preventDefault();
    e.stopPropagation();
    dragging = true;
    dragPointer = e.pointerId;
    dragStart = coord(e);
    dragStartScroll = position;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    if (!dragging || e.pointerId !== dragPointer) return;
    const range = track - thumbSize;
    if (range <= 0) return;
    onSeek(dragStartScroll + ((coord(e) - dragStart) / range) * maxScroll);
  }

  function onUp(e: PointerEvent) {
    if (!dragging || e.pointerId !== dragPointer) return;
    dragging = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      // capture may already be gone
    }
  }
</script>

{#if scrollable}
  <div class="track {orientation}">
    <div
      class="thumb"
      class:dragging
      class:shown
      style={orientation === "vertical"
        ? `top:${thumbPos}px; height:${thumbSize}px`
        : `left:${thumbPos}px; width:${thumbSize}px`}
      role="scrollbar"
      aria-controls={controls}
      aria-orientation={orientation}
      aria-valuenow={Math.round(fraction * 100)}
      aria-valuemin={0}
      aria-valuemax={100}
      tabindex="-1"
      onpointerdown={onDown}
      onpointermove={onMove}
      onpointerup={onUp}
      onpointercancel={onUp}
    ></div>
  </div>
{/if}

<style>
  /* The track floats over the content edge and never intercepts input; only
     the thumb itself is interactive. Sits above the panel resize handles
     (z-index 25) so grabbing the thumb wins where the two overlap. */
  .track {
    position: absolute;
    z-index: 26;
    pointer-events: none;
  }

  .track.vertical {
    top: 0;
    bottom: 0;
    right: 0;
    width: 8px;
  }

  .track.horizontal {
    left: 0;
    right: 0;
    bottom: 0;
    height: 8px;
  }

  .thumb {
    position: absolute;
    pointer-events: none;
    background: rgba(var(--accent-rgb), 0.5);
    border-radius: 8px;
    touch-action: none;
    opacity: 0;
    transition: opacity 140ms ease;
  }

  .thumb.shown {
    opacity: 1;
    pointer-events: auto;
  }

  .vertical .thumb {
    left: 2px;
    width: 4px;
  }

  .horizontal .thumb {
    top: 2px;
    height: 4px;
  }

  .thumb:hover,
  .thumb.dragging {
    background: rgba(var(--accent-rgb), 0.85);
  }
</style>
