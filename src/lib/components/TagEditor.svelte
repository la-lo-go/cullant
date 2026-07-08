<script lang="ts">
  import { api, type TaskTag } from "../api";
  import { tags } from "../stores/tags.svelte";

  let { onclose }: { onclose: () => void } = $props();

  let newName = $state("");
  let newScope = $state(2);
  let newColor = $state("#7a9bd6");
  /** Tag id currently listening for a shortcut key. */
  let recording = $state<number | null>(null);

  const scopeNames = ["Photos", "Videos", "Both"];

  async function create() {
    const name = newName.trim();
    if (!name) return;
    await api.createTaskTag(name, null, newScope, newColor);
    newName = "";
    await tags.refresh();
  }

  async function remove(tag: TaskTag) {
    await api.deleteTaskTag(tag.id);
    await tags.refresh();
  }

  async function setScope(tag: TaskTag, e: Event) {
    await api.updateTaskTag({ ...tag, scope: Number((e.target as HTMLSelectElement).value) });
    await tags.refresh();
  }

  function recordShortcut(tag: TaskTag) {
    recording = tag.id;
  }

  async function onKeydown(e: KeyboardEvent) {
    if (recording === null) return;
    e.preventDefault();
    e.stopPropagation();
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
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    tabindex="-1"
  >
    <header>
      <h2>Task tags</h2>
      <button onclick={onclose}>Close</button>
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
        <button class="del" title="Delete tag" onclick={() => remove(tag)}>🗑</button>
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
    background: #232329;
    border: 1px solid #3a3a42;
    border-radius: 10px;
    padding: 16px 20px;
    width: 480px;
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
    grid-template-columns: auto 1fr auto auto auto;
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
  }

  .key {
    font-family: Consolas, monospace;
    font-size: 12px;
    min-width: 70px;
  }

  .key.waiting {
    border-color: #6b6bff;
    color: #6bb2ff;
  }

  .create {
    display: flex;
    gap: 6px;
    align-items: center;
    border-top: 1px solid #2e2e36;
    padding-top: 10px;
  }

  .create input:not([type="color"]) {
    flex: 1;
  }

  input,
  select,
  button {
    border-radius: 6px;
    border: 1px solid #3a3a42;
    padding: 4px 8px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: #2a2a30;
  }

  button {
    cursor: pointer;
  }

  button:hover {
    border-color: #6b6bff;
  }

  .del {
    opacity: 0.6;
  }

  input[type="color"] {
    padding: 1px 2px;
    width: 34px;
    height: 26px;
  }
</style>
