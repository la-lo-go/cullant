import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { api } from "../api";
import { updateUrl } from "../update/channels";
import { isNewer, newestCompleteRelease } from "../update/release";

const SKIPPED_KEY = "cullant.skippedUpdate";

function skippedVersion(): string | null {
  try {
    return localStorage.getItem(SKIPPED_KEY);
  } catch {
    return null;
  }
}

class UpdateStore {
  current = $state<string | null>(null);
  latest = $state<string | null>(null);
  url = $state<string | null>(null);
  private skipped = $state<string | null>(skippedVersion());
  private dismissed = $state(false);
  private started = false;
  private atStartup = $state(false);

  available = $derived(
    this.current !== null && this.latest !== null && isNewer(this.latest, this.current),
  );
  prompt = $derived(this.available && this.atStartup && this.latest !== this.skipped && !this.dismissed);

  /**
   * One request per launch, however many callers ask. Only the startup call may
   * arm the prompt: Settings shows the result inline and must not open a modal.
   */
  async check(atStartup = false) {
    if (atStartup) this.atStartup = true;
    if (this.started) return;
    this.started = true;
    // The version shows even when the release lookup fails.
    this.current = await getVersion().catch(() => "unknown");
    try {
      const [source, latest] = await Promise.all([api.installSource(), newestCompleteRelease()]);
      this.url = updateUrl(source);
      this.latest = latest;
    } catch {
      // The check is optional. A failure shows nothing.
    }
  }

  skip() {
    this.skipped = this.latest;
    try {
      if (this.latest) localStorage.setItem(SKIPPED_KEY, this.latest);
    } catch { /* Storage is optional. */ }
  }

  later() {
    this.dismissed = true;
  }

  open() {
    if (this.url) void openUrl(this.url);
    this.later();
  }
}

export const update = new UpdateStore();
