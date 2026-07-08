import { session } from "../stores/session.svelte";
import { view } from "../stores/view.svelte";
import {
  effectiveBindings,
  loadOverrides,
  normalizeKey,
  saveOverrides,
  type CommandId,
} from "./keymap";

class KeymapStore {
  overrides = $state<Partial<Record<CommandId, string[]>>>(loadOverrides());
  bindings = $derived(effectiveBindings(this.overrides));
  /** When set, the next keydown rebinds this command instead of executing. */
  rebinding = $state<CommandId | null>(null);

  rebind(command: CommandId, key: string) {
    this.overrides = { ...this.overrides, [command]: [key] };
    saveOverrides(this.overrides);
  }

  reset() {
    this.overrides = {};
    saveOverrides(this.overrides);
  }
}

export const keymap = new KeymapStore();

function execute(id: CommandId, e: KeyboardEvent) {
  switch (id) {
    case "nav.next":
      return session.moveFocus(1);
    case "nav.prev":
      return session.moveFocus(-1);
    case "nav.down":
      return session.moveFocus(session.gridCols ?? 1);
    case "nav.up":
      return session.moveFocus(-(session.gridCols ?? 1));
    case "nav.home":
      return session.focusEdge(false);
    case "nav.end":
      return session.focusEdge(true);
    case "rate.0":
    case "rate.1":
    case "rate.2":
    case "rate.3":
    case "rate.4":
    case "rate.5":
      return session.rate(Number(id.slice(5)), e);
    case "flag.pick":
      return session.flag(1, e);
    case "flag.reject":
      return session.flag(-1, e);
    case "flag.unflag":
      return session.flag(0, e);
    case "flag.toggle":
      return session.toggleFlag(e);
    case "label.red":
      return session.label("Red", e);
    case "label.yellow":
      return session.label("Yellow", e);
    case "label.green":
      return session.label("Green", e);
    case "label.blue":
      return session.label("Blue", e);
    case "label.purple":
      return session.label("Purple", e);
    case "ui.toggleFilterBar":
      session.filterBarVisible = !session.filterBarVisible;
      return;
    case "ui.toggleMirror":
      session.mirrorMode = !session.mirrorMode;
      return;
    case "view.grid":
      view.mode = "grid";
      return;
    case "view.viewer":
      view.mode = "viewer";
      return;
    case "view.compare":
      view.mode = "compare";
      return;
    case "zoom.toggle":
      if (view.mode !== "grid") view.toggleZoom();
      return;
    case "pair.toggleShown":
      return session.togglePairHalf();
    case "pair.toggleCoupling":
      return void session.togglePairCoupling();
  }
}

export function handleKeydown(e: KeyboardEvent) {
  const target = e.target as HTMLElement | null;
  if (target && ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName)) return;

  const normalized = normalizeKey(e);

  if (keymap.rebinding) {
    if (normalized !== "escape") keymap.rebind(keymap.rebinding, normalized);
    keymap.rebinding = null;
    e.preventDefault();
    return;
  }

  const command = keymap.bindings.get(normalized);
  if (command) {
    e.preventDefault();
    execute(command, e);
  }
}
