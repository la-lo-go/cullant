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
  };
}
