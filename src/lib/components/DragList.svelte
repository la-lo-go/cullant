<script lang="ts" generics="Item">
  /**
   * Generic drag-to-reorder list with a grip handle per row. Pointer-based, so
   * it works with mouse and touch alike. The parent owns the array and applies
   * the move (via `onMove(from, to)`); this component only renders each row's
   * content through the `row` snippet and drives the reordering while dragging.
   */
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import type { Snippet } from "svelte";

  let {
    items,
    keyOf,
    onMove,
    row,
    ariaLabel = "Reorderable list",
  }: {
    items: Item[];
    keyOf: (item: Item, index: number) => string | number;
    onMove: (from: number, to: number) => void;
    row: Snippet<[Item, number]>;
    ariaLabel?: string;
  } = $props();

  let rowEls = $state<(HTMLElement | null)[]>([]);
  let dragIndex = $state(-1);
  let activePointer = -1;

  function startDrag(e: PointerEvent, i: number) {
    if (e.button !== 0 && e.pointerType === "mouse") return;
    dragIndex = i;
    activePointer = e.pointerId;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }

  function onMovePointer(e: PointerEvent) {
    if (dragIndex < 0 || e.pointerId !== activePointer) return;
    const y = e.clientY;
    // Walk the rows and settle on the slot whose midpoint the pointer has passed,
    // in the direction of travel — that becomes the new index for the dragged row.
    let target = dragIndex;
    for (let j = 0; j < items.length; j++) {
      const el = rowEls[j];
      if (!el) continue;
      const r = el.getBoundingClientRect();
      const mid = r.top + r.height / 2;
      if (j > dragIndex && y > mid) target = j;
      else if (j < dragIndex && y < mid) {
        target = j;
        break;
      }
    }
    if (target !== dragIndex) {
      onMove(dragIndex, target);
      dragIndex = target;
    }
  }

  function endDrag(e: PointerEvent) {
    if (e.pointerId !== activePointer) return;
    dragIndex = -1;
    activePointer = -1;
  }
</script>

<ul class="draglist" aria-label={ariaLabel}>
  {#each items as item, i (keyOf(item, i))}
    <li class="drow" class:dragging={dragIndex === i} bind:this={rowEls[i]}>
      <button
        class="handle"
        type="button"
        aria-label="Drag to reorder"
        onpointerdown={(e) => startDrag(e, i)}
        onpointermove={onMovePointer}
        onpointerup={endDrag}
        onpointercancel={endDrag}
      >
        <GripVertical size={15} />
      </button>
      <div class="content">
        {@render row(item, i)}
      </div>
    </li>
  {/each}
</ul>

<style>
  .draglist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .drow {
    display: flex;
    align-items: center;
    gap: 4px;
    border-radius: 6px;
  }

  .drow.dragging {
    background: var(--hover);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }

  .handle {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 28px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: #6a6a72;
    cursor: grab;
    /* The handle owns the drag gesture; stop the browser from scrolling/panning
       when a finger presses it. */
    touch-action: none;
  }

  .handle:hover {
    color: #bbb;
  }

  .drow.dragging .handle {
    cursor: grabbing;
    color: #ddd;
  }

  .content {
    flex: 1;
    min-width: 0;
  }
</style>
