/** App-wide user preferences, persisted in localStorage. */

import type { PreviewMode } from "../api";
export type { PreviewMode };

const PREVIEW_MODE_KEY = "cullant.previewMode";
const PROGRESSIVE_LOUPE_KEY = "cullant.progressiveLoupe";
const EDGE_TO_EDGE_KEY = "cullant.edgeToEdge";
const ONBOARDED_PREVIEW_KEY = "cullant.onboardedPreview";

function loadPreviewMode(): PreviewMode {
  try {
    const raw = localStorage.getItem(PREVIEW_MODE_KEY);
    if (raw === null) return "background";
    const v = JSON.parse(raw);
    return v === "all" || v === "window" ? v : "background";
  } catch {
    return "background";
  }
}

function loadBool(key: string, fallback: boolean): boolean {
  try {
    const raw = localStorage.getItem(key);
    return raw === null ? fallback : JSON.parse(raw) === true;
  } catch {
    return fallback;
  }
}

function save(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // persistence is best-effort
  }
}

class SettingsStore {
  /** How 2560px previews are pregenerated (applies on the next open/rescan). */
  previewMode = $state<PreviewMode>(loadPreviewMode());

  /** Paint the cached thumbnail instantly while the sharp preview loads. */
  progressiveLoupe = $state<boolean>(loadBool(PROGRESSIVE_LOUPE_KEY, true));

  /**
   * Edge-to-edge fullscreen (mainly Android). On: the app draws under the
   * status/navigation bars and display cutout, and only the controls that
   * touch a screen edge are padded by the safe-area insets. Off: the whole
   * app is inset so nothing ever sits under the system bars.
   */
  edgeToEdge = $state<boolean>(loadBool(EDGE_TO_EDGE_KEY, false));

  /**
   * Whether the one-time preview-mode intro has been shown and confirmed.
   * Gates the first interactive project open so the welcome dialog appears
   * exactly once, ever.
   */
  onboardedPreview = $state<boolean>(loadBool(ONBOARDED_PREVIEW_KEY, false));

  setPreviewMode(mode: PreviewMode) {
    this.previewMode = mode;
    save(PREVIEW_MODE_KEY, mode);
  }

  setProgressiveLoupe(on: boolean) {
    this.progressiveLoupe = on;
    save(PROGRESSIVE_LOUPE_KEY, on);
  }

  setEdgeToEdge(on: boolean) {
    this.edgeToEdge = on;
    save(EDGE_TO_EDGE_KEY, on);
  }

  setOnboardedPreview(on: boolean) {
    this.onboardedPreview = on;
    save(ONBOARDED_PREVIEW_KEY, on);
  }
}

export const settings = new SettingsStore();
