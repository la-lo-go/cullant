<script lang="ts">
  import { settings } from "../stores/settings.svelte";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Film from "@lucide/svelte/icons/film";
  import Video from "@lucide/svelte/icons/video";
  import History from "@lucide/svelte/icons/history";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";

  let {
    onclose,
    onshowkeybindings,
  }: {
    onclose: () => void;
    /** Open the keyboard-shortcuts dialog (owned by the page). */
    onshowkeybindings: () => void;
  } = $props();

  let panel = $state<HTMLDivElement | null>(null);

  // Focus the panel so Escape lands here (and stops) instead of the global keymap.
  $effect(() => {
    panel?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") onclose();
  }
</script>

<div
  class="backdrop"
  onclick={onclose}
  onkeydown={(e) => e.key === "Escape" && onclose()}
  role="presentation"
>
  <div
    class="dialog"
    bind:this={panel}
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    tabindex="-1"
  >
    <header class="head">
      <h2>Settings</h2>
    </header>

    <div class="content">
      <section class="card">
        <header class="card-head">
          <Monitor size={16} />
          <span class="card-text">
            <span class="card-title">Display</span>
            <span class="card-desc">How photos load and appear while you cull.</span>
          </span>
        </header>
        <label class="option">
          <input
            type="checkbox"
            checked={settings.progressiveLoupe}
            onchange={(e) => settings.setProgressiveLoupe(e.currentTarget.checked)}
          />
          <span class="text">
            <span class="label">Progressive loading</span>
            <span class="description">
              Paint the small thumbnail instantly (slightly soft for a moment) while the
              sharp preview loads. Off: keep showing the previous photo until the new
              preview is ready.
            </span>
          </span>
        </label>
        <label class="option">
          <input
            type="checkbox"
            checked={settings.dimQueuedDeletes}
            onchange={(e) => settings.setDimQueuedDeletes(e.currentTarget.checked)}
          />
          <span class="text">
            <span class="label">Dim thumbnails marked for deletion</span>
            <span class="description">
              Thumbnails queued for deletion render at reduced opacity in the grid and
              filmstrip, so rejects are easy to spot at a glance.
            </span>
          </span>
        </label>
      </section>

      <section class="card">
        <header class="card-head">
          <Film size={16} />
          <span class="card-text">
            <span class="card-title">Filmstrip badges</span>
            <span class="card-desc">
              The grid always keeps rating/label/tag badges inside the actual photo (a
              portrait thumbnail's letterbox gutters stay clear). The filmstrip's
              thumbnails are smaller, so its badges may sit outside the photo instead —
              pick which ones to show there.
            </span>
          </span>
        </header>
        <div class="checks">
          <label class="check">
            <input
              type="checkbox"
              checked={settings.filmstripShowType}
              onchange={(e) => settings.setFilmstripShowType(e.currentTarget.checked)}
            />
            <span>Photo type (RAW+JPG)</span>
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={settings.filmstripShowRating}
              onchange={(e) => settings.setFilmstripShowRating(e.currentTarget.checked)}
            />
            <span>Star rating</span>
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={settings.filmstripShowLabel}
              onchange={(e) => settings.setFilmstripShowLabel(e.currentTarget.checked)}
            />
            <span>Color label</span>
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={settings.filmstripShowFlag}
              onchange={(e) => settings.setFilmstripShowFlag(e.currentTarget.checked)}
            />
            <span>Pick/reject flag</span>
          </label>
          <label class="check">
            <input
              type="checkbox"
              checked={settings.filmstripShowTags}
              onchange={(e) => settings.setFilmstripShowTags(e.currentTarget.checked)}
            />
            <span>Tags</span>
          </label>
        </div>
      </section>

      <section class="card">
        <header class="card-head">
          <Video size={16} />
          <span class="card-text">
            <span class="card-title">Media</span>
            <span class="card-desc">Background generation of video poster frames.</span>
          </span>
        </header>
        <label class="option">
          <input
            type="checkbox"
            checked={settings.generateVideoThumbs}
            onchange={(e) => settings.setGenerateVideoThumbs(e.currentTarget.checked)}
          />
          <span class="text">
            <span class="label">Pregenerate video thumbnails</span>
            <span class="description">
              Video posters are generated last — only after every photo thumbnail and
              preview — because extracting them (via ffmpeg) is the slowest step. Off:
              skip generating them in the background; a video's poster is still made
              the moment you scroll to it.
            </span>
          </span>
        </label>
      </section>

      <section class="card">
        <header class="card-head">
          <History size={16} />
          <span class="card-text">
            <span class="card-title">Session</span>
            <span class="card-desc">What is restored when you reopen a project.</span>
          </span>
        </header>
        <label class="option">
          <input
            type="checkbox"
            checked={settings.rememberSession}
            onchange={(e) => settings.setRememberSession(e.currentTarget.checked)}
          />
          <span class="text">
            <span class="label">Remember per project</span>
            <span class="description">
              Reopen each project where you left off: restore the last sort order,
              media tab, active filters and focused photo. Off: every project opens
              with the defaults.
            </span>
          </span>
        </label>
      </section>

      <section class="card">
        <header class="card-head">
          <RefreshCw size={16} />
          <span class="card-text">
            <span class="card-title">Project folder</span>
            <span class="card-desc">Keeping the catalog in sync with files added or removed outside Cullant.</span>
          </span>
        </header>
        <div class="option row">
          <span class="text">
            <span class="label">Auto-rescan interval</span>
            <span class="description">
              How often to automatically rescan for added, removed or changed files.
              Only runs while the folder is reachable. You can always rescan now from
              the title-bar menu, or by pulling down the grid on touch.
            </span>
          </span>
          <select
            aria-label="Auto-rescan interval"
            onchange={(e) => settings.setAutoRescanMinutes(Number(e.currentTarget.value))}
          >
            <option value={0} selected={settings.autoRescanMinutes === 0}>Off</option>
            <option value={1} selected={settings.autoRescanMinutes === 1}>Every minute</option>
            <option value={5} selected={settings.autoRescanMinutes === 5}>Every 5 minutes</option>
            <option value={15} selected={settings.autoRescanMinutes === 15}>Every 15 minutes</option>
          </select>
        </div>
      </section>

      <section class="card">
        <header class="card-head">
          <Keyboard size={16} />
          <span class="card-text">
            <span class="card-title">Keyboard</span>
            <span class="card-desc">Review and remap every shortcut.</span>
          </span>
        </header>
        <button class="shortcuts" onclick={onshowkeybindings}>
          <Keyboard size={14} />
          <span>Keyboard shortcuts…</span>
        </button>
      </section>
    </div>

    <footer class="actions">
      <button class="close" onclick={onclose}>Close</button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
    /* Keep the centered panel inside the safe area (system bars, cutout). */
    padding: var(--inset-top) var(--inset-right) var(--inset-bottom) var(--inset-left);
    box-sizing: border-box;
  }

  .dialog {
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    width: 500px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    max-height: calc(100vh - 48px - var(--inset-top) - var(--inset-bottom));
    display: flex;
    flex-direction: column;
    outline: none;
  }

  .head {
    padding: 14px 20px 12px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    font-size: 16px;
  }

  /* Scrollable body: the header and footer stay put while the category cards
     scroll on small viewports. */
  .content {
    overflow-y: auto;
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  /* Each category is a card one shade darker than the dialog surface, so the
     grouping reads at a glance. */
  .card {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 10px 10px 6px;
  }

  .card-head {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 2px 10px 8px;
    color: var(--accent);
  }

  .card-head :global(svg) {
    flex-shrink: 0;
    margin-top: 1px;
  }

  .card-text {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .card-title {
    font-size: 13px;
    font-weight: 600;
    color: #e8e8e8;
  }

  .card-desc {
    font-size: 12px;
    opacity: 0.65;
    line-height: 1.4;
  }

  /* Setting rows are full-width and at least 44px tall so the whole row is a
     comfortable touch target on phones. */
  .option {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    min-height: 44px;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: pointer;
    box-sizing: border-box;
  }

  .option:hover {
    background: var(--hover);
  }

  /* A setting whose control sits inline at the right (e.g. a dropdown) rather
     than a leading checkbox. */
  .option.row {
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    cursor: default;
  }

  .option.row:hover {
    background: transparent;
  }

  .option.row .text {
    flex: 1;
    min-width: 0;
  }

  select {
    flex: none;
    min-height: 40px;
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 6px 8px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
  }

  select:hover {
    border-color: var(--accent);
  }

  .option input {
    margin-top: 2px;
  }

  input[type="checkbox"] {
    width: 16px;
    height: 16px;
    flex-shrink: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .label {
    font-size: 13px;
    font-weight: 600;
    line-height: 1.3;
  }

  .description {
    font-size: 12.5px;
    opacity: 0.65;
    line-height: 1.4;
  }

  /* Badge toggles stack vertically (mobile-first) instead of wrapping inline. */
  .checks {
    display: flex;
    flex-direction: column;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 4px 10px;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
    box-sizing: border-box;
  }

  .check:hover {
    background: var(--hover);
  }

  .shortcuts {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    width: 100%;
    min-height: 44px;
    margin-bottom: 4px;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    padding: 12px 20px;
    border-top: 1px solid var(--border);
  }

  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 8px 12px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }

  .close {
    min-width: 96px;
    min-height: 44px;
  }
</style>
