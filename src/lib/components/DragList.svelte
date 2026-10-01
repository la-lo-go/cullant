<script lang="ts" generics="Item">
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import { onDestroy, tick, type Snippet } from "svelte";
  import { flip } from "svelte/animate";

  let { items, keyOf, onMove, row, itemLabel, onInteractionChange, onPointerRelease, ariaLabel = "Reorderable list" }: {
    items: Item[];
    keyOf: (item: Item, index: number) => string | number;
    onMove: (from: number, to: number) => void;
    row: Snippet<[Item, number]>;
    itemLabel?: (item: Item, index: number) => string;
    onInteractionChange?: (active: boolean) => void;
    onPointerRelease?: () => void;
    ariaLabel?: string;
  } = $props();

  let list = $state<HTMLUListElement | null>(null);
  let dragKey = $state<string | number | null>(null);
  let dragOffset = $state(0);
  const dragIndex = $derived(items.findIndex((item, index) => keyOf(item, index) === dragKey));
  let activePointer = -1;
  let pointerY = 0;
  let grabOffset = 0;
  let frame = 0;
  let scroller: HTMLElement | null = null;
  const heldKeys = new Set<string>();
  let interacting = false;
  let keyboardMoving = false;

  function updateInteraction() {
    const active = activePointer !== -1 || heldKeys.size > 0;
    if (active === interacting) return;
    interacting = active;
    onInteractionChange?.(active);
  }

  function startDrag(e: PointerEvent, i: number) {
    if (e.button !== 0 && e.pointerType === "mouse") return;
    if (!list || activePointer !== -1) return;
    dragKey = keyOf(items[i], i);
    activePointer = e.pointerId;
    pointerY = e.clientY;
    grabOffset = e.clientY - list.children[i].getBoundingClientRect().top;
    scroller = list.parentElement;
    while (scroller && scroller.scrollHeight <= scroller.clientHeight) scroller = scroller.parentElement;
    // The list keeps capture when a keyed row moves to another position.
    list.setPointerCapture(e.pointerId);
    updateInteraction();
    e.preventDefault();
    frame = requestAnimationFrame(dragFrame);
  }

  function onMovePointer(e: PointerEvent) {
    if (dragIndex < 0 || e.pointerId !== activePointer) return;
    pointerY = e.clientY;
  }

  function targetIndex(): number {
    if (!list) return dragIndex;
    let target = dragIndex;
    const top = list.getBoundingClientRect().top;
    for (let j = 0; j < items.length; j++) {
      const el = list.children[j] as HTMLElement | undefined;
      if (!el) continue;
      // Layout coordinates do not change while FLIP animates another row.
      const mid = top + el.offsetTop + el.offsetHeight / 2;
      if (j > dragIndex && pointerY > mid) target = j;
      else if (j < dragIndex && pointerY < mid) { target = j; break; }
    }
    return target;
  }

  async function dragFrame() {
    if (!list || dragIndex < 0) return;
    if (scroller) {
      const bounds = scroller.getBoundingClientRect();
      if (pointerY < bounds.top + 36) scroller.scrollTop -= 8;
      else if (pointerY > bounds.bottom - 36) scroller.scrollTop += 8;
    }
    const target = targetIndex();
    if (target !== dragIndex) { onMove(dragIndex, target); await tick(); }
    if (!list || dragIndex < 0) return;
    const el = list.children[dragIndex] as HTMLElement;
    dragOffset = pointerY - grabOffset - list.getBoundingClientRect().top - el.offsetTop;
    frame = requestAnimationFrame(dragFrame);
  }

  function stopDrag() {
    cancelAnimationFrame(frame);
    if (list?.hasPointerCapture(activePointer)) list.releasePointerCapture(activePointer);
    dragKey = null;
    dragOffset = 0;
    activePointer = -1;
    scroller = null;
    updateInteraction();
  }

  function endDrag(e: PointerEvent) {
    if (e.pointerId !== activePointer) return;
    const handle = list?.children[dragIndex]?.querySelector<HTMLButtonElement>(".handle");
    stopDrag();
    handle?.blur();
    onPointerRelease?.();
  }

  async function moveByKeyboard(e: KeyboardEvent, from: number) {
    const delta = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
    if (delta === 0 || activePointer !== -1) return;
    e.preventDefault();
    e.stopPropagation();
    heldKeys.add(e.key);
    updateInteraction();
    const to = from + delta;
    if (to < 0 || to >= items.length) return;
    const control = e.currentTarget as HTMLButtonElement;
    keyboardMoving = true;
    onMove(from, to);
    await tick();
    if (control.isConnected) control.focus();
    keyboardMoving = false;
  }

  function releaseKey(e: KeyboardEvent) {
    heldKeys.delete(e.key);
    updateInteraction();
  }

  function endKeyboard() {
    // Moving a keyed row can briefly blur its handle before focus is restored.
    if (keyboardMoving) return;
    heldKeys.clear();
    updateInteraction();
  }

  function cancelInteraction() {
    heldKeys.clear();
    stopDrag();
  }

  onDestroy(cancelInteraction);
</script>

<svelte:window onblur={cancelInteraction} />

<ul class="draglist" class:reordering={dragIndex >= 0} aria-label={ariaLabel} bind:this={list}
  onpointermove={onMovePointer} onpointerup={endDrag} onpointercancel={endDrag} onlostpointercapture={endDrag}>
  {#each items as item, i (keyOf(item, i))}
    <li class="drow" class:dragging={dragIndex === i}
      style:translate={dragIndex === i ? `0 ${dragOffset}px` : null}
      animate:flip={{ duration: dragIndex === i ? 0 : 140 }}>
      <button class="handle" type="button" aria-label={itemLabel ? `Reorder ${itemLabel(item, i)}` : "Drag to reorder"}
        title="Drag to reorder. Use Up or Down when the handle has focus."
        onpointerdown={(e) => startDrag(e, i)} onkeydown={(e) => void moveByKeyboard(e, i)}
        onkeyup={releaseKey} onblur={endKeyboard}>
        <GripVertical size={15} />
      </button>
      <div class="content">{@render row(item, i)}</div>
    </li>
  {/each}
</ul>

<style>
  .draglist {
    position: relative;
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
    position: relative;
    z-index: 1;
    background: var(--surface);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.4);
  }
  .handle {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 40px;
    padding: 0;
    background: none;
    border: none;
    border-radius: 4px;
    color: #6a6a72;
    cursor: grab;
    touch-action: none;
  }
  .handle:focus-visible { color: var(--accent); }
  @media (hover: hover) {
    .handle:hover { color: var(--accent); }
  }
  .reordering { user-select: none; }
  .drow.dragging .handle { cursor: grabbing; color: #ddd; }
  .content { flex: 1; min-width: 0; }
  @media (prefers-reduced-motion: reduce) {
    .drow { animation-duration: 0s !important; }
  }
</style>
