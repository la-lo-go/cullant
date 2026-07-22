<script lang="ts">
  import { api, type TaskTag } from "../api";
  import { tags } from "../stores/tags.svelte";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import X from "@lucide/svelte/icons/x";

  let { onclose }: { onclose: () => void } = $props();

  let newName = $state("");
  let newScope = $state(2);
  let newColor = $state("#7a9bd6");
  let error = $state("");
  /** Tag id currently listening for a shortcut key. */
  let recording = $state<number | null>(null);

  const scopeNames = ["Photos", "Videos", "Both"];

  async function create() {
    const name = newName.trim();
    if (!name) return;
    error = "";
    try {
      await api.createTaskTag(name, null, newScope, newColor);
      newName = "";
      await tags.refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function remove(tag: TaskTag) {
    error = "";
    try {
      await api.deleteTaskTag(tag.id);
      await tags.refresh();
    } catch (e) {
      error = String(e);
    }
  }

  async function setScope(tag: TaskTag, e: Event) {
    error = "";
    try {
      await api.updateTaskTag({ ...tag, scope: Number((e.target as HTMLSelectElement).value) });
      await tags.refresh();
    } catch (err) {
      error = String(err);
    }
  }

  function recordShortcut(tag: TaskTag) {
    recording = tag.id;
  }

  async function onKeydown(e: KeyboardEvent) {
    if (recording === null) return;
    e.preventDefault();
    e.stopPropagation();
    // A lone modifier press is not a shortcut on its own — keep listening for
    // the next real key so we never store "ctrl+control".
    if (e.key === "Control" || e.key === "Alt" || e.key === "Shift" || e.key === "Meta") return;
    const tag = tags.all.find((t) => t.id === recording);
    recording = null;
    if (!tag || e.key === "Escape") return;
    const parts: string[] = [];
    if (e.ctrlKey) parts.push("ctrl");
    if (e.altKey) parts.push("alt");
    if (e.shiftKey) parts.push("shift");
    parts.push(e.key.toLowerCase());
    const shortcut = parts.join("+");
    await api.updateTaskTag({ ...tag, shortcut: e.key === "Backspace" ? null : shortcut });
    await tags.refresh();
  }

  // Escape closes the dialog (unless a shortcut is being recorded, where the
  // window capture handler above consumes it to cancel the recording instead).
  function onDialogKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape" && recording === null) onclose();
  }

  let dialogEl = $state<HTMLDivElement | null>(null);

  // Focus the panel on open so a key pressed before any click lands here and
  // stops, instead of reaching the global keymap and acting on the grid.
  $effect(() => {
    dialogEl?.focus();
  });
</script>

<svelte:window onkeydowncapture={onKeydown} />

<div
  class="backdrop"
  onclick={onclose}
  onkeydown={(e) => e.key === "Escape" && recording === null && onclose()}
  role="presentation"
>
  <div
    class="dialog"
    bind:this={dialogEl}
    onclick={(e) => e.stopPropagation()}
    onkeydown={onDialogKeydown}
    role="dialog"
    tabindex="-1"
  >
    <header>
      <h2>Task tags</h2>
      <button class="close-x" onclick={onclose} aria-label="Close" title="Close">
        <X size={18} />
      </button>
    </header>
    <p class="hint">
      Task tags mark work to do after culling (retouch, trim…). Assign a direct
      shortcut, or use the chord: <b>T</b> then <b>1-9</b> (list order). Backspace
      while recording clears a shortcut.
    </p>

    <div class="list">
      {#each tags.all as tag, i}
        <span class="dot" style="background: {tag.color ?? '#888'}"></span>
        <span class="name">{i + 1}. {tag.name}</span>
        <select value={String(tag.scope)} onchange={(e) => setScope(tag, e)}>
          {#each scopeNames as s, v}
            <option value={String(v)}>{s}</option>
          {/each}
        </select>
        <button class="key" class:waiting={recording === tag.id} onclick={() => recordShortcut(tag)}>
          {recording === tag.id ? "press key…" : (tag.shortcut ?? "—")}
        </button>
        <button class="del" title="Delete tag" onclick={() => remove(tag)}><Trash2 size={14} /></button>
      {/each}
    </div>

    <div class="create">
      <input placeholder="New tag name…" bind:value={newName} onkeydown={(e) => e.key === "Enter" && create()} />
      <select bind:value={newScope}>
        {#each scopeNames as s, v}
          <option value={v}>{s}</option>
        {/each}
      </select>
      <input type="color" bind:value={newColor} title="Tag color" />
      <button onclick={create}>Add</button>
    </div>
    {#if error}<p class="error">{error}</p>{/if}
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
    /* The dialog takes focus on open (so keys stop here); suppress the ring. */
    outline: none;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 16px 20px;
    /* border-box so the horizontal padding is included in max-width — otherwise
       the padding sits outside the cap and the panel overflows the viewport
       (clipped laterally) on narrow phones. */
    box-sizing: border-box;
    width: 480px;
    /* Honor the safe-area insets the backdrop pads with, so the cap matches the
       space actually available between the system bars/cutout. */
    max-width: calc(
      100vw - var(--dialog-edge-margin) * 2 - var(--inset-left) - var(--inset-right)
    );
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  header {
    display: flex;
    align-items: center;
  }

  h2 {
    margin: 0;
    font-size: 16px;
    flex: 1;
  }

  .hint {
    font-size: 12px;
    opacity: 0.6;
    margin: 0;
  }

  .list {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto auto auto;
    gap: 6px 10px;
    align-items: center;
    overflow-y: auto;
  }

  .dot {
    width: 12px;
    height: 12px;
    border-radius: 50%;
  }

  .name {
    font-size: 13px;
    /* Truncate long tag names instead of forcing the row wider than the panel. */
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .key {
    font-family: Consolas, monospace;
    font-size: 12px;
    min-width: 70px;
  }

  .key.waiting {
    border-color: var(--accent);
    color: var(--accent);
  }

  .create {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    align-items: center;
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }

  .create input:not([type="color"]) {
    flex: 1;
    /* Allow the text field to shrink below its content width so the row can
       fit (or wrap cleanly) on a narrow phone rather than overflowing. */
    min-width: 0;
  }

  input,
  select,
  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 4px 8px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
  }

  button {
    cursor: pointer;
  }

  button:hover {
    border-color: var(--accent);
  }

  /* Corner dismiss: borderless icon button, ≥40px hit area for touch. */
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
  }

  .close-x:hover {
    background: var(--hover);
    border: none;
    opacity: 1;
  }

  .del {
    display: inline-flex;
    align-items: center;
    opacity: 0.6;
  }

  .error {
    color: #ff6b6b;
    font-size: 12px;
    margin: 0;
  }

  input[type="color"] {
    padding: 1px 2px;
    width: 34px;
    height: 26px;
  }
</style>
