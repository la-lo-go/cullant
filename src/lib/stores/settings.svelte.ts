/** App-wide user preferences, persisted in localStorage. */

const PROGRESSIVE_LOUPE_KEY = "cullant.progressiveLoupe";
const REMEMBER_SESSION_KEY = "cullant.rememberSession";
const GENERATE_VIDEO_THUMBS_KEY = "cullant.generateVideoThumbs";

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
  /** Paint the cached thumbnail instantly while the sharp preview loads. */
  progressiveLoupe = $state<boolean>(loadBool(PROGRESSIVE_LOUPE_KEY, true));

  /** Remember and restore each project's last sort, media tab, filters and
   *  focused item (persisted in the per-project DB). */
  rememberSession = $state<boolean>(loadBool(REMEMBER_SESSION_KEY, true));

  /** Pregenerate video poster thumbnails. They always run last (after every
   *  photo thumbnail and preview) because ffmpeg extraction is the slow tier;
   *  off skips their background pregeneration entirely. Mirrored to the backend
   *  (see the sync effect in +page.svelte) since the ingest pass reads it. */
  generateVideoThumbs = $state<boolean>(loadBool(GENERATE_VIDEO_THUMBS_KEY, true));

  setProgressiveLoupe(on: boolean) {
    this.progressiveLoupe = on;
    save(PROGRESSIVE_LOUPE_KEY, on);
  }

  setRememberSession(on: boolean) {
    this.rememberSession = on;
    save(REMEMBER_SESSION_KEY, on);
  }

  setGenerateVideoThumbs(on: boolean) {
    this.generateVideoThumbs = on;
    save(GENERATE_VIDEO_THUMBS_KEY, on);
  }
}

export const settings = new SettingsStore();
