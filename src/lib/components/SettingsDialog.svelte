<script lang="ts">
  import { settings, type PreviewMode } from "../stores/settings.svelte";
  import { catalog } from "../stores/catalog.svelte";
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

  const previewModes: { value: PreviewMode; label: string; description: string }[] = [
    {
      value: "all",
      label: "All during import",
      description:
        "Previews are generated together with thumbnails while the project opens. " +
        "Slowest open, biggest cache — but browsing is instant from the first photo.",
    },
    {
      value: "background",
      label: "In background after opening",
      description:
        "The project opens as soon as thumbnails are ready; previews keep generating " +
        "behind a small progress chip. Photos you open jump the queue.",
    },
    {
      value: "window",
      label: "Around the focused photo",
      description:
        "No bulk generation: only photos near the one you are viewing are prepared. " +
        "Smallest cache — jumping far into an unvisited part may wait a moment.",
    },
  ];
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
      <h3>Preview generation</h3>
      <p class="hint">
        Previews are the sharp 2560px images the loupe and compare views show.
        Changes apply the next time a project is opened or rescanned.
      </p>
      {#each previewModes as m (m.value)}
        <label class="option">
          <input
            type="radio"
            name="preview-mode"
            value={m.value}
            checked={settings.previewMode === m.value}
            onchange={() => settings.setPreviewMode(m.value)}
          />
          <span class="text">
            <span class="label">{m.label}{m.value === "all" ? " (default)" : ""}</span>
            <span class="description">{m.description}</span>
          </span>
        </label>
      {/each}
    </section>

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
      <h3>Keyboard</h3>
      <button class="shortcuts" onclick={onshowkeybindings}>
        <Keyboard size={14} />
        <span>Keyboard shortcuts…</span>
      </button>
    </section>

    <div class="actions">
      {#if catalog.project}
        <span class="note">Preview mode applies on the next open or rescan.</span>
      {/if}
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
  }

  .dialog {
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 16px 20px;
    width: 460px;
    max-width: calc(100vw - 24px);
    max-height: calc(100vh - 48px);
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

  .hint {
    margin: 0 0 8px;
    font-size: 12px;
    opacity: 0.6;
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

  .note {
    font-size: 11px;
    opacity: 0.5;
    margin-right: auto;
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
