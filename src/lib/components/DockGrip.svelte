<script lang="ts">
  /**
   * The handle of a dockable panel. Drag it to move the panel (the drag state
   * goes to `dock.drag`, which `DockGuides` draws). Click it for a position menu.
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
    /** The panel's own orientation. */
    orientation: "horizontal" | "vertical";
    /** A label that becomes the handle. The grip icon then fades in after it on hover. */
    children?: Snippet;
  }

  const { panel, target, thicknessFor, orientation, children }: Props = $props();

  const DRAG_THRESHOLD = 5;
  const MAX_SHARE = 0.5;
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

  function track(e: PointerEvent) {
    const region = target()?.getBoundingClientRect();
    if (!region) return cancel();
    dock.drag = {
      panel,
      region,
      x: e.clientX,
      y: e.clientY,
      candidate: pickSide(region, e.clientX, e.clientY, DOCK_SIDES[panel]),
      // The preview and the compass never promise more than half of the region.
      thickness: (side) =>
        Math.min(thicknessFor(side), MAX_SHARE * (isVertical(side) ? region.width : region.height)),
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
    dragging = false;
    listen(false);
    if (held && button?.hasPointerCapture(held.id)) button.releasePointerCapture(held.id);
    if (dock.drag?.panel === panel) dock.drag = null;
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

<button
  bind:this={button}
  class="dock-grip"
  class:dragging
  class:labelled={!!children}
  type="button"
  aria-label={`Move ${DOCK_LABELS[panel].toLowerCase()}`}
  title="Drag to move. Click to choose a position."
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={cancel}
  onlostpointercapture={cancel}
  {onclick}
>
  <!-- A horizontal panel is a bar, so its handle is a short vertical strip of dots. -->
  {#snippet icon()}
    {#if orientation === "horizontal"}
      <GripVertical size={12} />
    {:else}
      <GripHorizontal size={12} />
    {/if}
  {/snippet}
  {#if children}
    {@render children()}
    <span class="after">{@render icon()}</span>
  {:else}
    {@render icon()}
  {/if}
</button>

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
    border-radius: 4px;
    background: transparent;
    color: inherit;
    opacity: 0.45;
    cursor: grab;
    touch-action: none;
  }
  .dock-grip:hover,
  .dock-grip:focus-visible,
  .dock-grip.dragging {
    opacity: 1;
  }
  .dock-grip.dragging {
    cursor: grabbing;
  }
  @media (pointer: coarse) {
    .dock-grip {
      width: 28px;
      height: 28px;
    }
    .dock-grip.labelled {
      width: auto;
      height: auto;
      min-height: 28px;
    }
  }
  .dock-grip.labelled {
    width: auto;
    height: auto;
    gap: 6px;
    padding: 0;
    border-radius: 0;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    opacity: 0.85;
  }
  .after {
    display: inline-flex;
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .labelled:hover .after,
  .labelled:focus-visible .after,
  .labelled.dragging .after {
    opacity: 0.6;
  }
  @media (pointer: coarse) {
    .after {
      opacity: 0.5;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .after {
      transition: none;
    }
  }
</style>
