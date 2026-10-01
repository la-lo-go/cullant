interface ModalOptions {
  initialFocus?: string;
  isRecording?: () => boolean;
  onKeyboardInteraction?: () => void;
}

interface ModalEntry {
  panel: HTMLElement;
  restoreFocus: HTMLElement | null;
  isRecording?: () => boolean;
  onKeyboardInteraction?: () => void;
}

const modals: ModalEntry[] = [];
const inertElements = new Map<HTMLElement, boolean>();
const FOCUSABLE = 'a[href], button, input, select, textarea, [tabindex], [contenteditable="true"]';
let observer: MutationObserver | null = null;

function activeModal(): ModalEntry | undefined {
  return modals.at(-1);
}

function focusable(panel: HTMLElement): HTMLElement[] {
  return [...panel.querySelectorAll<HTMLElement>(FOCUSABLE)].filter(
    (el) => el.tabIndex >= 0 && !el.matches(':disabled') && !el.closest('[inert]') && el.getClientRects().length > 0,
  );
}

function restoreInert() {
  for (const [el, inert] of inertElements) el.inert = inert;
  inertElements.clear();
}

function isolateModal() {
  restoreInert();
  let branch: HTMLElement | null = activeModal()?.panel ?? null;
  while (branch && branch !== document.body) {
    const parent: HTMLElement | null = branch.parentElement;
    if (!parent) break;
    for (const sibling of parent.children) {
      if (!(sibling instanceof HTMLElement) || sibling === branch || sibling.getAttribute('role') === 'tooltip') continue;
      inertElements.set(sibling, sibling.inert);
      sibling.inert = true;
    }
    branch = parent;
  }
}

function containFocus(e: FocusEvent) {
  const modal = activeModal();
  if (modal && !(e.target instanceof Node && modal.panel.contains(e.target))) modal.panel.focus({ preventScroll: true });
}

function containKeys(e: KeyboardEvent) {
  const modal = activeModal();
  if (!modal) return;
  modal.onKeyboardInteraction?.();
  if (!(e.target instanceof Node && modal.panel.contains(e.target))) {
    e.preventDefault();
    e.stopImmediatePropagation();
    modal.panel.focus({ preventScroll: true });
    return;
  }
  if (e.key !== 'Tab' || modal.isRecording?.()) return;
  e.preventDefault();
  e.stopImmediatePropagation();
  const controls = focusable(modal.panel);
  const index = controls.indexOf(document.activeElement as HTMLElement);
  const next = e.shiftKey ? (index <= 0 ? controls.length - 1 : index - 1) : (index + 1) % controls.length;
  (controls[next] ?? modal.panel).focus({ preventScroll: true });
}

function stopGlobalKeys(e: KeyboardEvent) {
  if (activeModal()) e.stopPropagation();
}

/** Keep modal focus and keys in the top panel. Restore the app when it closes. */
export function modalFocus(panel: HTMLElement, options: ModalOptions = {}) {
  const modal: ModalEntry = {
    panel,
    restoreFocus: document.activeElement instanceof HTMLElement ? document.activeElement : null,
    isRecording: options.isRecording,
    onKeyboardInteraction: options.onKeyboardInteraction,
  };
  const previousAriaModal = panel.getAttribute('aria-modal');
  const previousTabindex = panel.getAttribute('tabindex');
  panel.setAttribute('aria-modal', 'true');
  if (previousTabindex === null) panel.tabIndex = -1;
  modals.push(modal);
  if (modals.length === 1) {
    window.addEventListener('keydown', containKeys, true);
    document.addEventListener('keydown', stopGlobalKeys);
    document.addEventListener('focusin', containFocus, true);
    observer = new MutationObserver(() => {
      isolateModal();
      const current = activeModal();
      if (current && !current.panel.contains(document.activeElement)) current.panel.focus({ preventScroll: true });
    });
    observer.observe(document.body, { childList: true, subtree: true });
  }
  isolateModal();
  const initialFocus = options.initialFocus ? panel.querySelector<HTMLElement>(options.initialFocus) : null;
  (initialFocus ?? panel).focus({ preventScroll: true });

  return {
    destroy() {
      const wasActive = activeModal() === modal;
      modals.splice(modals.indexOf(modal), 1);
      isolateModal();
      if (previousAriaModal === null) panel.removeAttribute('aria-modal');
      else panel.setAttribute('aria-modal', previousAriaModal);
      if (previousTabindex === null) panel.removeAttribute('tabindex');
      if (modals.length === 0) {
        observer?.disconnect();
        observer = null;
        window.removeEventListener('keydown', containKeys, true);
        document.removeEventListener('keydown', stopGlobalKeys);
        document.removeEventListener('focusin', containFocus, true);
      }
      if (!wasActive) return;
      queueMicrotask(() => {
        const target = modal.restoreFocus;
        const current = activeModal();
        if (target?.isConnected && !target.closest('[inert]') && (!current || current.panel.contains(target))) {
          target.focus({ preventScroll: true });
        } else current?.panel.focus({ preventScroll: true });
      });
    },
  };
}
