let dismissCurrent: (() => void) | null = null;
let nextId = 0;

function positionTooltip(tooltip: HTMLElement, target: HTMLElement) {
  const anchor = target.getBoundingClientRect();
  const bounds = tooltip.getBoundingClientRect();
  const viewport = window.visualViewport;
  const left = viewport?.offsetLeft ?? 0;
  const top = viewport?.offsetTop ?? 0;
  const width = viewport?.width ?? window.innerWidth;
  const height = viewport?.height ?? window.innerHeight;
  const x = Math.max(left + 8, Math.min(anchor.left + (anchor.width - bounds.width) / 2, left + width - bounds.width - 8));
  const above = anchor.top - bounds.height - 8;
  const y = above >= top + 8 ? above : Math.min(anchor.bottom + 8, top + height - bounds.height - 8);
  tooltip.style.left = `${x}px`;
  tooltip.style.top = `${Math.max(top + 8, y)}px`;
}

/** Shared button help: hover, keyboard focus, or touch hold without an action. */
export function tooltips(root: HTMLElement) {
  const controller = new AbortController();
  const signal = controller.signal;
  let target: HTMLElement | null = null;
  let tooltip: HTMLElement | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let nativeTitle: string | null = null;
  let keyboard = false;
  let press: { target: HTMLElement; id: number; x: number; y: number } | null = null;
  let suppressClick: HTMLElement | null = null;

  function control(eventTarget: EventTarget | null) {
    if (!(eventTarget instanceof Element)) return null;
    const element = eventTarget.closest<HTMLElement>('button, [role="img"]');
    return element && root.contains(element) ? element : null;
  }

  function hide() {
    clearTimeout(timer);
    if (target && nativeTitle !== null && !target.hasAttribute("title")) target.title = nativeTitle;
    if (target && tooltip) {
      const descriptions = (target.getAttribute("aria-describedby") ?? "").split(/\s+/).filter(id => id && id !== tooltip!.id);
      if (descriptions.length) target.setAttribute("aria-describedby", descriptions.join(" "));
      else target.removeAttribute("aria-describedby");
    }
    tooltip?.remove();
    tooltip = null;
    target = null;
    nativeTitle = null;
    if (dismissCurrent === hide) dismissCurrent = null;
  }

  function show(text: string) {
    if (!target?.isConnected || target.getBoundingClientRect().width === 0) return hide();
    tooltip = document.createElement("div");
    tooltip.id = `button-tooltip-${++nextId}`;
    tooltip.setAttribute("role", "tooltip");
    tooltip.textContent = text;
    tooltip.style.cssText = "position:fixed;z-index:1000;pointer-events:none;max-width:min(320px,calc(100vw - 24px));padding:7px 10px;border:1px solid var(--border-strong);border-radius:6px;background:var(--surface-2);color:inherit;font-family:inherit;font-size:12px;line-height:1.4;box-shadow:0 3px 12px var(--bg-stage);overflow-wrap:anywhere;";
    document.body.append(tooltip);
    const description = target.getAttribute("aria-describedby");
    target.setAttribute("aria-describedby", [description, tooltip.id].filter(Boolean).join(" "));
    positionTooltip(tooltip, target);
  }

  function schedule(element: HTMLElement, delay: number, held = false) {
    dismissCurrent?.();
    const text = (element.title || element.getAttribute("aria-label") || element.textContent || "").trim();
    if (!text) return;
    target = element;
    nativeTitle = element.getAttribute("title");
    // Keep the native fallback outside interaction; avoid two hover bubbles.
    element.removeAttribute("title");
    dismissCurrent = hide;
    timer = setTimeout(() => {
      show(text);
      if (held && tooltip) suppressClick = element;
    }, delay);
  }

  function pointerDown(event: PointerEvent) {
    keyboard = false;
    suppressClick = null;
    press = null;
    hide();
    const element = control(event.target);
    if (!element || event.pointerType === "mouse" || !event.isPrimary || event.button !== 0) return;
    press = { target: element, id: event.pointerId, x: event.clientX, y: event.clientY };
    schedule(element, 550, true);
  }

  function pointerMove(event: PointerEvent) {
    if (!press || event.pointerId !== press.id) return;
    if (Math.hypot(event.clientX - press.x, event.clientY - press.y) <= 8) return;
    suppressClick = press.target;
    press = null;
    hide();
  }

  function pointerUp(event: PointerEvent) {
    if (!press || event.pointerId !== press.id) return;
    press = null;
    if (tooltip) timer = setTimeout(hide, 1800);
    else hide();
  }

  function pointerOver(event: PointerEvent) {
    if (event.pointerType === "touch" || event.buttons) return;
    const element = control(event.target);
    if (element && element !== target) schedule(element, 450);
  }

  function pointerOut(event: PointerEvent) {
    if (event.pointerType === "touch" || press || !target) return;
    if (event.relatedTarget instanceof Node && target.contains(event.relatedTarget)) return;
    if (control(event.target) === target) hide();
  }

  function keyDown(event: KeyboardEvent) {
    keyboard = true;
    if (event.key === "Escape" && tooltip) {
      event.preventDefault();
      event.stopImmediatePropagation();
      hide();
    } else if (event.key !== "Tab") hide();
  }

  function click(event: MouseEvent) {
    if (suppressClick && control(event.target) === suppressClick) {
      event.preventDefault();
      event.stopImmediatePropagation();
      suppressClick.blur();
      suppressClick = null;
    } else hide();
  }

  root.addEventListener("pointerover", pointerOver, { signal });
  root.addEventListener("pointerout", pointerOut, { signal });
  root.addEventListener("focusin", event => {
    const element = control(event.target);
    if (keyboard && element) schedule(element, 0);
  }, { signal });
  root.addEventListener("focusout", () => { if (keyboard) hide(); }, { signal });
  root.addEventListener("click", click, { capture: true, signal });
  root.addEventListener("contextmenu", event => {
    if (press || suppressClick) event.preventDefault();
  }, { capture: true, signal });
  document.addEventListener("pointerdown", pointerDown, { capture: true, signal });
  window.addEventListener("pointermove", pointerMove, { capture: true, passive: true, signal });
  window.addEventListener("pointerup", pointerUp, { capture: true, signal });
  window.addEventListener("pointercancel", () => { press = null; hide(); }, { capture: true, signal });
  window.addEventListener("keydown", keyDown, { capture: true, signal });
  window.addEventListener("scroll", hide, { capture: true, passive: true, signal });
  window.addEventListener("resize", hide, { signal });
  window.addEventListener("blur", hide, { signal });
  window.visualViewport?.addEventListener("resize", hide, { signal });
  document.addEventListener("visibilitychange", hide, { signal });
  const observer = new MutationObserver(() => {
    if (target && !target.isConnected) hide();
  });
  observer.observe(root, { childList: true, subtree: true });

  return {
    destroy() {
      controller.abort();
      observer.disconnect();
      hide();
    },
  };
}
