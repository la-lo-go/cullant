export function folderContextMenu(node: HTMLButtonElement, onmenu: (x: number, y: number) => void) {
  let open = onmenu;
  let holdTimer: ReturnType<typeof setTimeout> | undefined;
  let held = false;
  let holdX = 0;
  let holdY = 0;
  const controller = new AbortController();
  const options = { signal: controller.signal };

  function cancelHold() {
    clearTimeout(holdTimer);
    holdTimer = undefined;
  }

  node.addEventListener("pointerdown", (event) => {
    cancelHold();
    held = false;
    if (event.pointerType === "mouse" || !event.isPrimary) return;
    holdX = event.clientX;
    holdY = event.clientY;
    holdTimer = setTimeout(() => { held = true; open(holdX, holdY); }, 500);
  }, options);
  node.addEventListener("pointermove", (event) => {
    if (Math.hypot(event.clientX - holdX, event.clientY - holdY) > 10) cancelHold();
  }, options);
  node.addEventListener("pointerup", cancelHold, options);
  node.addEventListener("pointerleave", cancelHold, options);
  node.addEventListener("pointercancel", () => { cancelHold(); held = false; }, options);
  node.addEventListener("contextmenu", (event) => {
    event.preventDefault();
    cancelHold();
    open(event.clientX, event.clientY);
  }, options);
  node.addEventListener("keydown", (event) => {
    if (event.key !== "ContextMenu" && !(event.shiftKey && event.key === "F10")) return;
    event.preventDefault();
    event.stopPropagation();
    const bounds = node.getBoundingClientRect();
    open(bounds.left, bounds.bottom);
  }, options);
  node.addEventListener("click", (event) => {
    cancelHold();
    if (held && event.detail !== 0) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
    held = false;
  }, { ...options, capture: true });

  return {
    update(next: typeof onmenu) { open = next; },
    destroy() { cancelHold(); controller.abort(); },
  };
}
