<script module lang="ts">
  let nextId = 0;
</script>

<script lang="ts">
  import type { Snippet } from "svelte";
  import OverlayScrollbar from "./OverlayScrollbar.svelte";

  // A vertical scroll container with the overlay bar wired up, so hosts need
  // no scroll-metric code. The wrapper takes the host's layout (`class` and
  // `style`: size, flex, max-height). The host keeps its padding and inner
  // layout on its own element inside `children`.
  let {
    element = $bindable(null),
    class: className = "",
    style = "",
    onscroll,
    children,
  }: {
    /** The scroll container, for hosts that read or set scrollTop. */
    element?: HTMLDivElement | null;
    class?: string;
    style?: string;
    onscroll?: (e: Event) => void;
    children: Snippet;
  } = $props();

  const id = `scroll-area-${nextId++}`;
  let inner = $state<HTMLDivElement | null>(null);
  let scrollTop = $state(0);
  let viewport = $state(0);
  let content = $state(0);

  // scrollHeight and clientHeight change with the content, not only with the
  // box, so observe both elements.
  $effect(() => {
    const el = element;
    if (!el || !inner) return;
    const measure = () => {
      viewport = el.clientHeight;
      content = el.scrollHeight;
      scrollTop = el.scrollTop;
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    ro.observe(inner);
    return () => ro.disconnect();
  });
</script>

<div class="scroll-area {className}" {style}>
  <div
    class="scroller"
    {id}
    bind:this={element}
    onscroll={(e) => {
      scrollTop = e.currentTarget.scrollTop;
      onscroll?.(e);
    }}
  >
    <div bind:this={inner}>{@render children()}</div>
  </div>
  <OverlayScrollbar
    orientation="vertical"
    {viewport}
    {content}
    position={scrollTop}
    controls={id}
    onSeek={(pos) => {
      if (element) element.scrollTop = pos;
    }}
  />
</div>

<style>
  .scroll-area {
    position: relative;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .scroller {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    /* A native bar would carve its gutter out of the content width. */
    scrollbar-width: none;
  }

  .scroller::-webkit-scrollbar {
    display: none;
  }
</style>
