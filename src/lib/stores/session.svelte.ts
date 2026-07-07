import { listen } from "@tauri-apps/api/event";
import { api, type CullState, type ItemLite, type Targets } from "../api";
import { catalog } from "./catalog.svelte";

export type FlagFilter = "all" | "pick" | "reject" | "unflagged";

export const LABELS = ["Red", "Yellow", "Green", "Blue", "Purple"] as const;
export type Label = (typeof LABELS)[number];

class SessionStore {
  // --- filters ---
  flagFilter = $state<FlagFilter>("all");
  minRating = $state(0);
  labelFilter = $state<string | null>(null);
  filterBarVisible = $state(true);

  // --- mirror mode (M4 flips the display; fan-out is live already) ---
  mirrorMode = $state(true);

  // --- focus / selection (indexes into `filtered`) ---
  focusedIndex = $state(0);
  /** Column count reported by the grid so ↑/↓ move one visual row. */
  gridCols = $state(1);

  // Auto-advance: Lightroom semantics. Caps Lock ON advances after any
  // classification; Shift inverts the behaviour one-shot.
  autoAdvancePref = $state(false);

  filtered = $derived.by(() => {
    let out = catalog.items;
    if (this.flagFilter !== "all") {
      const want = this.flagFilter === "pick" ? 1 : this.flagFilter === "reject" ? -1 : 0;
      out = out.filter((i) => i.flag === want);
    }
    if (this.minRating > 0) {
      out = out.filter((i) => i.rating >= this.minRating);
    }
    if (this.labelFilter !== null) {
      out = out.filter((i) => i.label === this.labelFilter);
    }
    return out;
  });

  focused = $derived<ItemLite | undefined>(this.filtered[this.focusedIndex]);

  counts = $derived.by(() => {
    let pick = 0;
    let reject = 0;
    let unflagged = 0;
    for (const i of catalog.items) {
      if (i.flag === 1) pick++;
      else if (i.flag === -1) reject++;
      else unflagged++;
    }
    return { pick, reject, unflagged, total: catalog.items.length };
  });

  clampFocus() {
    const max = Math.max(0, this.filtered.length - 1);
    if (this.focusedIndex > max) this.focusedIndex = max;
    if (this.focusedIndex < 0) this.focusedIndex = 0;
  }

  moveFocus(delta: number) {
    const max = Math.max(0, this.filtered.length - 1);
    this.focusedIndex = Math.min(max, Math.max(0, this.focusedIndex + delta));
  }

  focusEdge(end: boolean) {
    this.focusedIndex = end ? Math.max(0, this.filtered.length - 1) : 0;
  }

  private targets(): Targets | null {
    const item = this.focused;
    if (!item) return null;
    return { ids: [item.id], asGroups: this.mirrorMode };
  }

  /// Merge authoritative rows back into the catalog (optimistic UI included:
  /// the same merge applies local guesses and server truth).
  applyStates(states: CullState[]) {
    if (states.length === 0) return;
    const byId = new Map(states.map((s) => [s.id, s]));
    for (const item of catalog.items) {
      const s = byId.get(item.id);
      if (s) {
        item.rating = s.rating;
        item.flag = s.flag;
        item.label = s.label;
      }
    }
  }

  private maybeAdvance(event?: KeyboardEvent) {
    const caps = event?.getModifierState("CapsLock") ?? false;
    const shift = event?.shiftKey ?? false;
    const advance = (caps || this.autoAdvancePref) !== shift; // XOR: shift inverts
    if (advance) this.moveFocus(1);
  }

  async rate(rating: number, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    this.applyStates(t.ids.map((id) => this.localGuess(id, { rating })));
    this.maybeAdvance(event);
    this.applyStates(await api.setRating(t, rating));
  }

  async flag(flag: number, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    this.applyStates(t.ids.map((id) => this.localGuess(id, { flag })));
    this.maybeAdvance(event);
    this.applyStates(await api.setFlag(t, flag));
  }

  async toggleFlag(event?: KeyboardEvent) {
    const item = this.focused;
    if (!item) return;
    await this.flag(item.flag === 1 ? 0 : 1, event);
  }

  async label(label: string | null, event?: KeyboardEvent) {
    const t = this.targets();
    if (!t) return;
    const item = this.focused;
    // Pressing the same label key again clears it (Lightroom behaviour).
    const next = item && item.label === label ? null : label;
    this.applyStates(t.ids.map((id) => this.localGuess(id, { label: next })));
    this.maybeAdvance(event);
    this.applyStates(await api.setLabel(t, next));
  }

  private localGuess(id: number, patch: Partial<CullState>): CullState {
    const current = catalog.items.find((i) => i.id === id);
    return {
      id,
      rating: patch.rating ?? current?.rating ?? 0,
      flag: patch.flag ?? current?.flag ?? 0,
      label: "label" in patch ? (patch.label ?? null) : (current?.label ?? null),
    };
  }
}

export const session = new SessionStore();

// Multi-window / background changes reconcile through the same merge.
listen<CullState[]>("state:changed", (e) => session.applyStates(e.payload));
