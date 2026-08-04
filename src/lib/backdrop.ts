/**
 * Dismiss handlers for a modal/popover backdrop.
 *
 * A bare `onclick={close}` closes on *any* click that reaches the backdrop,
 * including one the browser put there rather than the user: a click is
 * dispatched on the nearest common ancestor of the press and release targets,
 * so a panel that moves or re-renders between `pointerdown` and `pointerup`
 * hands its click to the backdrop. Both happen routinely while a project is
 * loading — the toolbar reflows as the file counts arrive, and panel sections
 * appear and disappear as the catalog is replaced — and read to the user as the
 * panel closing by itself.
 *
 * Requiring the press *and* the release to land on the backdrop keeps
 * click-outside working while making a retargeted click a no-op.
 *
 * A right-click needs its own handler for two reasons: Windows fires no `click`
 * for the right button, so `onclick` never sees it and the panel would stay
 * open; and the backdrop is covering something the user is pointing at, so
 * closing is not enough — the press is handed on to whatever is underneath.
 * That is what makes right-clicking a photo work while a toolbar popover is
 * open, and what lets one context menu reopen on another photo.
 *
 * Build it once in `<script>` (it keeps state between the two events) and spread
 * it onto the backdrop element:
 *
 * ```svelte
 * const dismiss = backdropDismiss(onclose);
 * <div class="backdrop" {...dismiss}></div>
 * ```
 */
export function backdropDismiss(close: () => void) {
  let pressedOnBackdrop = false;
  return {
    onpointerdown(e: PointerEvent) {
      pressedOnBackdrop = e.target === e.currentTarget;
    },
    onclick(e: MouseEvent) {
      const dismiss = pressedOnBackdrop && e.target === e.currentTarget;
      pressedOnBackdrop = false;
      if (dismiss) close();
    },
    oncontextmenu(e: MouseEvent) {
      if (e.target !== e.currentTarget) return;
      e.preventDefault();
      const backdrop = e.currentTarget as HTMLElement;
      // elementFromPoint would answer "the backdrop" while it is still
      // hit-testable, so lift it for the length of the query.
      backdrop.style.pointerEvents = "none";
      const under = document.elementFromPoint(e.clientX, e.clientY);
      backdrop.style.pointerEvents = "";
      close();
      // Stacked backdrops cascade: each one closes and passes the press down
      // until it reaches the element that has something to say about it.
      under?.dispatchEvent(
        new MouseEvent("contextmenu", {
          bubbles: true,
          cancelable: true,
          clientX: e.clientX,
          clientY: e.clientY,
        }),
      );
    },
  };
}
