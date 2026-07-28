<script lang="ts">
  import {
    settings,
    BURST_GAP_CHOICES,
    PREVIEW_QUALITY_CHOICES,
    PREVIEW_QUALITY_LABELS,
  } from "../stores/settings.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import { session } from "../stores/session.svelte";
  import { api, type DeletionMode } from "../api";
  import { backdropDismiss } from "../backdrop";
  import type { BurstMode } from "../bursts";
  import DragList from "./DragList.svelte";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Layers from "@lucide/svelte/icons/layers";
  import Keyboard from "@lucide/svelte/icons/keyboard";
  import Monitor from "@lucide/svelte/icons/monitor";
  import Film from "@lucide/svelte/icons/film";
  import Video from "@lucide/svelte/icons/video";
  import History from "@lucide/svelte/icons/history";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import Zap from "@lucide/svelte/icons/zap";
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import RotateCcw from "@lucide/svelte/icons/rotate-ccw";
  import Tag from "@lucide/svelte/icons/tag";
  import X from "@lucide/svelte/icons/x";

  let {
    onclose,
    onshowkeybindings,
    onshowtags,
  }: {
    onclose: () => void;
    /** Open the keyboard-shortcuts dialog (owned by the page). */
    onshowkeybindings: () => void;
    /** Open the task-tag editor (owned by the page). */
    onshowtags: () => void;
  } = $props();

  // Touch platform (Android) reaches the manual rescan via pull-to-refresh;
  // desktop uses the title-bar menu. The auto-rescan help text reflects whichever
  // one this device actually has.
  const isTouch = navigator.userAgent.includes("Android");

  const dismiss = backdropDismiss(() => onclose());

  let panel = $state<HTMLDivElement | null>(null);

  // Focus the panel so Escape lands here (and stops) instead of the global keymap.
  $effect(() => {
    panel?.focus();
  });

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") onclose();
  }

  // Deletion mode is stored per project, so this card only exists with one open
  // — Settings is also reachable from the home screen. The commit dialog offers
  // the same control and writes the same key.
  let deletionMode = $state<DeletionMode>("trash");
  let deletionError = $state("");

  $effect(() => {
    if (!catalog.project) return;
    void api.getProjectSetting("deletionMode").then(
      (v) => (deletionMode = v === "permanent" ? "permanent" : "trash"),
      () => {},
    );
  });

  // Preview quality invalidates every generated preview, so the select never
  // applies straight away: it parks the choice here and waits for a confirm.
  let pendingQuality = $state<number | null>(null);
  let previewBusy = $state(false);
  let previewMsg = $state("");

  function changePreviewQuality(e: Event) {
    const select = e.currentTarget as HTMLSelectElement;
    const next = Number(select.value);
    if (next === settings.previewQuality) return;
    // Keep showing the value still in force until the change is confirmed.
    select.value = String(settings.previewQuality);
    pendingQuality = next;
  }

  async function applyPreviewQuality() {
    const next = pendingQuality;
    pendingQuality = null;
    if (next === null) return;
    previewBusy = true;
    previewMsg = "";
    try {
      settings.setPreviewQuality(next);
      await api.setPreviewQuality(next);
      if (!catalog.project) {
        previewMsg = "Saved. Previews are rebuilt the next time you open a project.";
        return;
      }
      // Best-effort: if this fails (a scan is running), the previews are merely
      // left on disk. The ingest regenerates them anyway, because a row whose
      // long_edge no longer matches the setting counts as missing.
      await api.discardPreviews();
      await api.rescanProject();
      previewMsg = "Rebuilding previews at the new size…";
    } catch (err) {
      previewMsg = String(err);
    } finally {
      previewBusy = false;
    }
  }

  async function changeDeletionMode(e: Event) {
    const previous = deletionMode;
    deletionMode = (e.currentTarget as HTMLSelectElement).value as DeletionMode;
    deletionError = "";
    try {
      await api.setProjectSetting("deletionMode", deletionMode);
    } catch (err) {
      deletionMode = previous;
      deletionError = String(err);
    }
  }
</script>

<div
  class="backdrop"
  {...dismiss}
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
      <button class="close-x" onclick={onclose} aria-label="Close settings" title="Close">
        <X size={18} />
      </button>
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
              Show the thumbnail instantly while the sharp preview loads.
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
              Fade rejects in the grid and filmstrip so they stand out at a glance.
            </span>
          </span>
        </label>
        <div class="option row">
          <span class="text">
            <span class="label">Preview quality</span>
            <span class="description">
              How sharp the loupe preview is. Lower is faster to generate and uses far less
              memory — worth it on a phone. Changing this regenerates every preview.
            </span>
          </span>
          <select
            aria-label="Preview quality"
            disabled={previewBusy}
            value={settings.previewQuality}
            onchange={changePreviewQuality}
          >
            {#each PREVIEW_QUALITY_CHOICES as choice (choice)}
              <option value={choice}>{choice} px · {PREVIEW_QUALITY_LABELS[choice]}</option>
            {/each}
          </select>
        </div>
        {#if previewMsg}
          <p class="hint">{previewMsg}</p>
        {/if}
      </section>

      <section class="card">
        <header class="card-head">
          <Zap size={16} />
          <span class="card-text">
            <span class="card-title">Culling</span>
            <span class="card-desc">Faster keyboard/touch culling in the loupe and compare views.</span>
          </span>
        </header>
        <label class="option">
          <input
            type="checkbox"
            checked={settings.fastCulling}
            onchange={(e) => settings.setFastCulling(e.currentTarget.checked)}
          />
          <span class="text">
            <span class="label">Fast culling</span>
            <span class="description">
              In the loupe and compare views, any rating, flag, label or tag jumps to the
              next photo automatically. Hold Shift to stay put.
            </span>
          </span>
        </label>
        <label class="option">
          <input
            type="checkbox"
            checked={settings.lockCarousel}
            onchange={(e) => settings.setLockCarousel(e.currentTarget.checked)}
          />
          <span class="text">
            <span class="label">Lock carousel</span>
            <span class="description">
              Scrolling the filmstrip moves the loupe to the centered photo, instead of
              scrolling on its own.
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
              Which badges to show on the filmstrip's small thumbnails.
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
          <PanelBottom size={16} />
          <span class="card-text">
            <span class="card-title">Bottom action bar</span>
            <span class="card-desc">
              Drag to reorder the touch classification groups; uncheck one to hide it.
            </span>
          </span>
        </header>
        <div class="bar-list">
          <DragList
            items={settings.bottomBarList}
            keyOf={(it) => it.id}
            onMove={(from, to) => settings.moveBottomBarItem(from, to)}
            ariaLabel="Bottom bar groups"
          >
            {#snippet row(it)}
              <label class="bar-row">
                <span class="bar-name" class:off={it.hidden}>{it.label}</span>
                <input
                  type="checkbox"
                  checked={!it.hidden}
                  onchange={() => settings.toggleBottomBarHidden(it.id)}
                />
              </label>
            {/snippet}
          </DragList>
        </div>
        <button class="reset" onclick={() => settings.resetBottomBar()}>
          <RotateCcw size={13} />
          <span>Reset to default</span>
        </button>
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
              Build video posters in the background. Off: each is made when you scroll to it.
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
              Restore each project's last sort, filters and focused photo.
            </span>
          </span>
        </label>
      </section>

      <section class="card">
        <header class="card-head">
          <RefreshCw size={16} />
          <span class="card-text">
            <span class="card-title">Project folder</span>
            <span class="card-desc">Keep the catalog in sync with files changed outside Cullant.</span>
          </span>
        </header>
        <div class="option row">
          <span class="text">
            <span class="label">Auto-rescan interval</span>
            <span class="description">
              How often to rescan for added or removed files, when the folder is reachable.
              {#if isTouch}
                Pull down the grid to rescan now.
              {:else}
                Rescan now from the title-bar menu.
              {/if}
            </span>
          </span>
          <select
            aria-label="Auto-rescan interval"
            onchange={(e) => settings.setAutoRescanMinutes(Number(e.currentTarget.value))}
          >
            <option value={0} selected={settings.autoRescanMinutes === 0}>Off</option>
            <option value={1} selected={settings.autoRescanMinutes === 1}>1 minute</option>
            <option value={5} selected={settings.autoRescanMinutes === 5}>5 minutes</option>
            <option value={15} selected={settings.autoRescanMinutes === 15}>15 minutes</option>
          </select>
        </div>
      </section>

      {#if catalog.project}
        <section class="card">
          <header class="card-head">
            <Layers size={16} />
            <span class="card-text">
              <span class="card-title">Bursts</span>
              <span class="card-desc">How close together shots must be to count as one burst.</span>
            </span>
          </header>
          <div class="option row">
            <span class="text">
              <span class="label">Threshold</span>
              <span class="description">
                {#if settings.burstMode === "adaptive"}
                  {#if session.burstGap.adaptive}
                    Read from this project's own rhythm: {session.burstGap.seconds}s.
                  {:else}
                    This project's intervals show no clear split, so the fixed gap is in use.
                  {/if}
                {:else}
                  Shots separated by less than this belong to the same burst.
                {/if}
              </span>
            </span>
            <select
              aria-label="Burst threshold mode"
              value={settings.burstMode}
              onchange={(e) => settings.setBurstMode(e.currentTarget.value as BurstMode)}
            >
              <option value="fixed">Fixed gap</option>
              <option value="adaptive">Adaptive</option>
            </select>
          </div>
          <div class="option row">
            <span class="text">
              <span class="label">Fixed gap</span>
              <span class="description">
                Used directly in fixed mode, and as the fallback when adaptive finds no clear
                split.
              </span>
            </span>
            <select
              aria-label="Burst gap in seconds"
              onchange={(e) => settings.setBurstGapSeconds(Number(e.currentTarget.value))}
            >
              {#each BURST_GAP_CHOICES as choice (choice)}
                <option value={choice} selected={settings.burstGapSeconds === choice}>
                  {choice} second{choice === 1 ? "" : "s"}
                </option>
              {/each}
            </select>
          </div>
          <p class="hint">
            A burst never spans two cameras, and a RAW+JPEG pair always stays together.
          </p>
        </section>

        <section class="card">
          <header class="card-head">
            <Trash2 size={16} />
            <span class="card-text">
              <span class="card-title">Deletion</span>
              <span class="card-desc">Where files go when a queued delete is committed.</span>
            </span>
          </header>
          <div class="option row">
            <span class="text">
              <span class="label">Deleted files go to</span>
              <span class="description">
                {#if deletionMode === "permanent"}
                  Files are erased outright. This cannot be undone, and they do not reach the
                  Recycle Bin.
                {:else}
                  A _trash folder inside the project, mirroring the original subfolders — so a
                  delete stays reversible.
                {/if}
              </span>
            </span>
            <select aria-label="Deletion mode" value={deletionMode} onchange={changeDeletionMode}>
              <option value="trash">Project _trash folder</option>
              <option value="permanent">Permanent (no undo!)</option>
            </select>
          </div>
          {#if deletionError}
            <p class="hint">{deletionError}</p>
          {/if}
        </section>
      {/if}

      <section class="card">
        <header class="card-head">
          <Tag size={16} />
          <span class="card-text">
            <span class="card-title">Task tags</span>
            <span class="card-desc">The to-do labels you can put on a photo or clip.</span>
          </span>
        </header>
        <button class="shortcuts" onclick={onshowtags}>
          <Tag size={14} />
          <span>Edit task tags…</span>
        </button>
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
        <p class="hint">Press ? anytime to see the shortcuts you have configured.</p>
      </section>
    </div>
  </div>
</div>

{#if pendingQuality !== null}
  <ConfirmDialog
    title="Rebuild every preview?"
    message={catalog.project
      ? `Loupe previews will be regenerated at ${pendingQuality} px. The ones built at ${settings.previewQuality} px are deleted first, so photos you open before this finishes show their grid thumbnail for a moment. Your photos are not touched.`
      : `Previews will be generated at ${pendingQuality} px from now on. Existing projects rebuild theirs the next time you open them.`}
    confirmLabel="Rebuild previews"
    onconfirm={() => void applyPreviewQuality()}
    oncancel={() => (pendingQuality = null)}
  />
{/if}

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
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 12px 20px;
    border-bottom: 1px solid var(--border);
  }

  h2 {
    margin: 0;
    font-size: 16px;
    flex: 1;
  }

  /* Corner dismiss: a borderless icon button, ≥40px hit area for touch. */
  .close-x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    flex: none;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: inherit;
    opacity: 0.7;
    cursor: pointer;
  }

  .close-x:hover {
    background: var(--hover);
    opacity: 1;
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
    /* Neutral, not the card-head's accent (which the icon uses) — descriptions
       shouldn't read as colored links. */
    color: #e8e8e8;
    opacity: 0.55;
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

  /* Box/chevron come from the app-wide :global(select); keep only the taller
     touch target and right padding for the chevron. */
  select {
    flex: none;
    min-height: 40px;
    padding: 6px 28px 6px 10px;
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

  /* No lingering focus ring on the last-tapped control (checkbox / select /
     button) — matches the app-wide button rule, extended here to the inputs
     the settings dialog uses. */
  input:focus,
  input:focus-visible,
  select:focus,
  select:focus-visible,
  button:focus,
  button:focus-visible {
    outline: none;
    box-shadow: none;
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

  /* Bottom-bar customization: the DragList rows carry the grip; each row here is
     the item name + a show/hide checkbox. */
  .bar-list {
    padding: 2px 6px 6px;
  }

  .bar-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    min-height: 40px;
    padding: 4px 6px;
    cursor: pointer;
  }

  .bar-name {
    font-size: 13px;
  }

  .bar-name.off {
    opacity: 0.5;
  }

  .reset {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin: 0 6px 4px;
    font-size: 12.5px;
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

  .hint {
    margin: 2px 2px 6px;
    font-size: 12.5px;
    opacity: 0.65;
    line-height: 1.4;
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
