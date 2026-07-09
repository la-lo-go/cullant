/** App-wide user preferences, persisted in localStorage. */

import type { PreviewMode } from "../api";
export type { PreviewMode };

const PREVIEW_MODE_KEY = "cullant.previewMode";
const PROGRESSIVE_LOUPE_KEY = "cullant.progressiveLoupe";

function loadPreviewMode(): PreviewMode {
  try {
    const raw = localStorage.getItem(PREVIEW_MODE_KEY);
    if (raw === null) return "all";
    const v = JSON.parse(raw);
    return v === "background" || v === "window" ? v : "all";
  } catch {
    return "all";
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

  setPreviewMode(mode: PreviewMode) {
    this.previewMode = mode;
    save(PREVIEW_MODE_KEY, mode);
  }

  setProgressiveLoupe(on: boolean) {
    this.progressiveLoupe = on;
    save(PROGRESSIVE_LOUPE_KEY, on);
  }
}

export const settings = new SettingsStore();
