<script lang="ts">
  /**
   * The handle of a dockable panel. Drag it to move the panel. The drag state goes to
   * `dock.drag`: `DockGuides` draws the compass, and `DockGhost` draws the ghost and the
   * frame. Click it for a position menu.
   *
   * The host places the grip and styles it through the class `dock-grip`.
   * Pointer capture keeps every move event on this button, so the hosts under the
   * pointer never see hover. `pointerdown` stops at the grip so a host such as a
   * filmstrip cell or a resize handle does not react to it.
   */
  import type { Snippet } from "svelte";
  import GripVertical from "@lucide/svelte/icons/grip-vertical";
  import GripHorizontal from "@lucide/svelte/icons/grip-horizontal";
  import PanelTop from "@lucide/svelte/icons/panel-top";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import ContextMenu from "./ContextMenu.svelte";
  import type { MenuNode } from "../menu";
  import {
    DOCK_LABELS,
    DOCK_SIDES,
    SIDE_LABELS,
    dock,
    isVertical,
    pickSide,
    type DockPanel,
    type Side,
  } from "../stores/dock.svelte";

  interface Props {
    panel: DockPanel;
    /** The dock region: the element whose edges the panel can attach to. */
    target: () => HTMLElement | null;
    /** Size in px of the panel if it is docked at that side. */
    thicknessFor: (side: Side) => number;
    /** The orientation of the row, bar or tab the grip sits in. The dots run across it. */
    orientation: "horizontal" | "vertical";
    /** A label that becomes the handle. The grip dots stay visible before it, and the grip fills its row. */
    children?: Snippet;
    /** True while the panel floats over the region, so the region is its whole box, padding included. */
    floats?: () => boolean;
    /** A look-alike that does nothing: the grip of a ghost keeps the geometry of the real one. */
    still?: boolean;
  }

  const { panel, target, thicknessFor, orientation, children, still = false, floats }: Props = $props();

  const describedBy = $derived(`dock-grip-help-${panel}`);
  const DRAG_THRESHOLD = 5;
  // Rough height of the menu, to open it above a grip in the lower half of the window.
  const MENU_ROW = 28;
  const MENU_CHROME = 44;
  const SIDE_ICONS = { top: PanelTop, right: PanelRight, bottom: PanelBottom, left: PanelLeft };

  let button = $state<HTMLButtonElement | null>(null);
  let press: { id: number; x: number; y: number } | null = null;
  let dragging = $state(false);
  let menu = $state<{ x: number; y: number; items: MenuNode[] } | null>(null);

  function openMenu() {
    const rect = button?.getBoundingClientRect();
    if (!rect) return;
    const current = dock[panel];
    const height = DOCK_SIDES[panel].length * MENU_ROW + MENU_CHROME;
    const above = rect.bottom > window.innerHeight / 2;
    menu = {
      x: rect.left,
      y: above ? Math.max(8, rect.top - 4 - height) : rect.bottom + 4,
      items: [
        { kind: "header", label: `${DOCK_LABELS[panel]} position` },
        ...DOCK_SIDES[panel].map((side): MenuNode => ({
          kind: "item",
          label: SIDE_LABELS[side],
          icon: SIDE_ICONS[side],
          checked: side === current,
          run: () => dock.set(panel, side),
        })),
      ],
    };
  }

  // The region without its padding: a panel sits inside it, so the preview must too.
  function contentBox(el: HTMLElement): DOMRect {
    const rect = el.getBoundingClientRect();
    const style = getComputedStyle(el);
    const inset = (side: string) =>
      parseFloat(style.getPropertyValue(`padding-${side}`)) +
      parseFloat(style.getPropertyValue(`border-${side}-width`));
    const [left, top] = [inset("left"), inset("top")];
    return new DOMRect(
      rect.left + left,
      rect.top + top,
      rect.width - left - inset("right"),
      rect.height - top - inset("bottom"),
    );
  }

  function track(e: PointerEvent) {
    const el = target();
    if (!el) return cancel();
    const region = floats?.() ? el.getBoundingClientRect() : contentBox(el);
    dock.drag = {
      panel,
      region,
      candidate: pickSide(region, e.clientX, e.clientY, DOCK_SIDES[panel]),
      // The size the panel lands with, which the gap and the ghost use too. Only the region caps it.
      thickness: (side) => Math.min(thicknessFor(side), isVertical(side) ? region.width : region.height),
    };
  }

  // Capture phase and `stopImmediatePropagation`: Escape must not reach
  // `view.back`, and a letter must not switch the view in the middle of a drag.
  function swallowKey(e: KeyboardEvent) {
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.key === "Escape") cancel();
  }

  function listen(on: boolean) {
    if (on) {
      window.addEventListener("keydown", swallowKey, true);
      window.addEventListener("blur", cancel);
    } else {
      window.removeEventListener("keydown", swallowKey, true);
      window.removeEventListener("blur", cancel);
    }
  }

  function end() {
    const held = press;
    press = null;
    const was = dragging;
    dragging = false;
    if (was) dock.onCancel(null);
    listen(false);
    if (held && button?.hasPointerCapture(held.id)) button.releasePointerCapture(held.id);
    // Read before the drag ends. A drop on a new side no longer vacates (the panel is
    // there). A cancel does, and then the slot and the gap move back together.
    const leaving = dock.vacating(panel);
    if (dock.drag?.panel === panel) dock.drag = null;
    if (leaving) dock.beginReturn(panel);
    button?.blur();
  }

  function cancel() {
    if (press) end();
  }

  function onpointerdown(e: PointerEvent) {
    e.stopPropagation();
    if (press || e.button !== 0) return;
    e.preventDefault();
    press = { id: e.pointerId, x: e.clientX, y: e.clientY };
    button?.setPointerCapture(e.pointerId);
    listen(true);
  }

  function onpointermove(e: PointerEvent) {
    if (!press || e.pointerId !== press.id) return;
    if (!dragging) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) <= DRAG_THRESHOLD) return;
      dragging = true;
      dock.onCancel(cancel);
      dock.endReturn(panel);
    }
    track(e);
  }

  function onpointerup(e: PointerEvent) {
    if (!press || e.pointerId !== press.id) return;
    const wasClick = !dragging;
    const side = dock.drag?.candidate;
    if (!wasClick && side) dock.set(panel, side);
    end();
    if (wasClick) openMenu();
  }

  // Detail 0 is a keyboard activation; a pointer click already opened the menu.
  function onclick(e: MouseEvent) {
    if (e.detail === 0) openMenu();
  }

  $effect(() => cancel);
</script>

<!-- The dots run across the row the grip sits in. -->
{#snippet content()}
  <span class="dots" aria-hidden="true">
    {#if orientation === "horizontal"}
      <GripVertical size={12} />
    {:else}
      <GripHorizontal size={12} />
    {/if}
  </span>
  {@render children?.()}
{/snippet}

{#if still}
  <span class="dock-grip" class:labelled={!!children}>{@render content()}</span>
{:else}
  <button
    bind:this={button}
    class="dock-grip"
    class:dragging
    class:labelled={!!children}
    type="button"
    aria-label={children ? undefined : `Move ${DOCK_LABELS[panel].toLowerCase()}`}
    aria-describedby={children ? describedBy : undefined}
    title="Drag to move. Click to choose a position."
    {onpointerdown}
    {onpointermove}
    {onpointerup}
    onpointercancel={cancel}
    onlostpointercapture={cancel}
    {onclick}
  >
    {@render content()}
  </button>
{/if}

{#if children && !still}
  <!-- A hidden element can still describe the button. -->
  <span id={describedBy} hidden>
    Move {DOCK_LABELS[panel].toLowerCase()}. Click to choose a position.
  </span>
{/if}

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .dock-grip {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: none;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: var(--r-sm);
    background: transparent;
    color: inherit;
    cursor: grab;
    touch-action: none;
  }
  .dock-grip.dragging {
    cursor: grabbing;
  }
  .dock-grip:hover,
  .dock-grip:focus-visible,
  .dock-grip.dragging {
    background: var(--hover);
  }
  @media (pointer: coarse) {
    .dock-grip {
      width: 28px;
      height: 28px;
    }
    .dock-grip.labelled {
      min-height: 28px;
    }
  }
  /* The grip is the whole row: it stretches over its flex parent, and the host
     sets the padding that places the label. */
  .dock-grip.labelled {
    width: auto;
    height: auto;
    flex: 1;
    align-self: stretch;
    justify-content: flex-start;
    gap: 5px;
    padding: 0;
    border-radius: 0;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    text-align: left;
    transition: background 120ms ease, color 120ms ease;
  }
  .dock-grip.labelled:hover,
  .dock-grip.labelled:focus-visible,
  .dock-grip.labelled.dragging {
    color: #ccc;
  }
  .dock-grip.labelled.dragging {
    background: var(--accent-fill);
    color: #fff;
  }

  /* Three levels for the dots, the same on every panel: faint at rest, stronger while
     the pointer is over the panel (a host puts the class dock-panel on its root), full
     on the grip. A touch screen has no hover, so it keeps one middle level. */
  .dots {
    display: inline-flex;
    opacity: 0.28;
    transition: opacity 120ms ease;
  }
  :global(.dock-panel:hover) .dots {
    opacity: 0.55;
  }
  .dock-grip:hover .dots,
  .dock-grip:focus-visible .dots,
  .dock-grip.dragging .dots {
    opacity: 1;
  }
  @media (pointer: coarse) {
    .dots,
    :global(.dock-panel:hover) .dots,
    .dock-grip:hover .dots {
      opacity: 0.5;
    }
    .dock-grip:focus-visible .dots,
    .dock-grip.dragging .dots {
      opacity: 1;
    }
  }
</style>
