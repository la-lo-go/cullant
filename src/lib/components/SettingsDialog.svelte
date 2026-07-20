<script lang="ts">
  import { settings } from "../stores/settings.svelte";
  import Keyboard from "@lucide/svelte/icons/keyboard";

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
    <h2>Settings</h2>

    <section>
      <h3>Loupe display</h3>
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
    </section>

    <section>
      <h3>Session</h3>
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

    <section>
      <h3>Media</h3>
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

    <section>
      <h3>Filmstrip badges</h3>
      <p class="hint">
        The grid always keeps rating/label/tag badges inside the actual photo (a
        portrait thumbnail's letterbox gutters stay clear). The filmstrip's
        thumbnails are smaller, so its badges may sit outside the photo instead —
        pick which ones to show there.
      </p>
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

    <section>
      <h3>Keyboard</h3>
      <button class="shortcuts" onclick={onshowkeybindings}>
        <Keyboard size={14} />
        <span>Keyboard shortcuts…</span>
      </button>
    </section>

    <div class="actions">
      <button class="close" onclick={onclose}>Close</button>
    </div>
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
    padding: 16px 20px;
    width: 460px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    max-height: calc(100vh - 48px - var(--inset-top) - var(--inset-bottom));
    overflow-y: auto;
    outline: none;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
  }

  h3 {
    margin: 14px 0 4px;
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    opacity: 0.55;
  }

  .option {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: pointer;
  }

  .option:hover {
    background: var(--hover);
  }

  .option input {
    margin-top: 3px;
    accent-color: var(--accent);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .label {
    font-size: 13px;
    font-weight: 600;
  }

  .description {
    font-size: 12px;
    opacity: 0.65;
    line-height: 1.4;
  }

  .hint {
    margin: 0 0 6px;
    padding: 0 10px;
    font-size: 12px;
    opacity: 0.65;
    line-height: 1.4;
  }

  .checks {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    padding: 2px 10px 6px;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    cursor: pointer;
  }

  .check input {
    accent-color: var(--accent);
  }

  .shortcuts {
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 10px;
    margin-top: 16px;
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
</style>
