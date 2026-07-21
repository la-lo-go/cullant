<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount } from "svelte";
  import {
    api,
    type CommitOutcome,
    type CommitPlan,
    type DeletionMode,
    type PendingAction,
  } from "../api";
  import { session } from "../stores/session.svelte";
  import { catalog } from "../stores/catalog.svelte";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import FolderInput from "@lucide/svelte/icons/folder-input";
  import Copy from "@lucide/svelte/icons/copy";
  import Tag from "@lucide/svelte/icons/tag";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import X from "@lucide/svelte/icons/x";

  let plan = $state<CommitPlan | null>(null);
  let running = $state(false);
  let progress = $state({ done: 0, total: 0 });
  let outcome = $state<CommitOutcome | null>(null);
  let error = $state("");
  let expanded = $state<"deletes" | "moves" | "copies" | null>(null);

  const deletionModeNames: Record<DeletionMode, string> = {
    permanent: "Permanent delete",
    trash: "Project _trash folder",
  };

  async function refresh() {
    error = "";
    try {
      plan = await api.commitPreview();
    } catch (e) {
      error = String(e);
    }
  }

  async function changeMode(e: Event) {
    const mode = (e.target as HTMLSelectElement).value;
    await api.setProjectSetting("deletionMode", mode);
    await refresh();
  }

  async function execute() {
    if (!plan) return;
    running = true;
    outcome = null;
    error = "";
    try {
      outcome = await api.commitExecute(plan.planHash);
      await catalog.refresh();
      await session.refreshPending();
      plan = await api.commitPreview();
    } catch (e) {
      error = String(e);
      await refresh();
    } finally {
      running = false;
    }
  }

  async function unqueue(row: PlanRow, isDelete: boolean) {
    await api.removePending(row.ids);
    // Deletes mirror the reject flag: unqueueing a delete also un-rejects the
    // files, keeping flag and queue in sync in both directions. Rows are already
    // per-file expanded, hence asGroups: false. Moves/copies leave flags alone.
    if (isDelete) await api.setFlag({ ids: row.fileIds, asGroups: false }, 0);
    await session.refreshPending();
    await refresh();
  }

  // A single line in an expanded action list. `ids` are the pending-action ids
  // it stands for — two when a RAW+JPEG pair is collapsed into one line, so its
  // unqueue button removes both halves at once. `fileIds` are the underlying
  // files, needed to clear the reject flag when a delete is unqueued.
  interface PlanRow {
    label: string;
    ids: number[];
    fileIds: number[];
    dest: string | null;
  }

  function isJpegExt(ext: string): boolean {
    const e = ext.toLowerCase();
    return e === "jpg" || e === "jpeg";
  }

  // Split a relPath into its directory+stem key and its extension (no dot).
  function splitName(relPath: string): { key: string; ext: string } {
    const slash = relPath.lastIndexOf("/");
    const name = relPath.slice(slash + 1);
    const dir = relPath.slice(0, slash + 1); // trailing slash, or "" at root
    const dot = name.lastIndexOf(".");
    const stem = dot > 0 ? name.slice(0, dot) : name;
    const ext = dot > 0 ? name.slice(dot + 1) : "";
    return { key: dir + stem, ext };
  }

  // Collapse a RAW+JPEG pair queued for the SAME action into one line
  // ("AJDJ8378.CR3+jpg"). Only collapses when exactly two items share a
  // basename+dir (and destination) and one is a JPEG while the other is not;
  // a lone RAW or lone JPEG stays a normal line.
  function planRows(items: PendingAction[]): PlanRow[] {
    const groups = new Map<string, PendingAction[]>();
    const order: string[] = [];
    for (const p of items) {
      const gk = `${splitName(p.relPath).key}\u0000${p.dest ?? ""}`;
      let g = groups.get(gk);
      if (!g) {
        g = [];
        groups.set(gk, g);
        order.push(gk);
      }
      g.push(p);
    }
    const rows: PlanRow[] = [];
    for (const gk of order) {
      const g = groups.get(gk)!;
      if (g.length === 2) {
        const jpeg = g.find((p) => isJpegExt(splitName(p.relPath).ext));
        const raw = g.find((p) => !isJpegExt(splitName(p.relPath).ext));
        if (jpeg && raw) {
          rows.push({
            label: `${raw.relPath}+${splitName(jpeg.relPath).ext.toLowerCase()}`,
            ids: [raw.id, jpeg.id],
            fileIds: [raw.fileId, jpeg.fileId],
            dest: raw.dest,
          });
          continue;
        }
      }
      for (const p of g)
        rows.push({ label: p.relPath, ids: [p.id], fileIds: [p.fileId], dest: p.dest });
    }
    return rows;
  }

  function close() {
    session.commitDialogOpen = false;
  }

  onMount(() => {
    refresh();
    let unlisten: UnlistenFn | undefined;
    listen<{ done: number; total: number }>("commit:progress", (e) => {
      progress = e.payload;
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  });

  const total = $derived(
    plan ? plan.deletes.length + plan.moves.length + plan.copies.length + plan.xmpCount : 0
  );

  const deleteRows = $derived(plan ? planRows(plan.deletes) : []);
  const moveRows = $derived(plan ? planRows(plan.moves) : []);
  const copyRows = $derived(plan ? planRows(plan.copies) : []);
</script>

<div
  class="backdrop"
  onclick={close}
  onkeydown={(e) => e.key === "Escape" && close()}
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
      <h2>Commit pending actions</h2>
      <button class="close-x" onclick={close} aria-label="Close" title="Close">
        <X size={18} />
      </button>
    </header>

    {#if plan}
      {#if plan.conflicts.length > 0}
        <div class="conflicts">
          <TriangleAlert size={14} style="vertical-align: -2px" />
          {plan.conflicts.length} conflict(s):
          <ul>
            {#each plan.conflicts.slice(0, 5) as c}<li>{c}</li>{/each}
          </ul>
        </div>
      {/if}

      <div class="rows">
        <button
          class="row"
          disabled={plan.deletes.length === 0}
          onclick={() => (expanded = expanded === "deletes" ? null : "deletes")}
        >
          <span class="icon"><Trash2 size={16} /></span>
          <span class="what">Delete {plan.deletes.length} file(s)</span>
          <span class="how">{deletionModeNames[plan.deletionMode]}</span>
        </button>
        {#if expanded === "deletes"}
          <ul class="detail">
            {#each deleteRows as row}
              <li>
                {row.label}
                <button class="unqueue" title="Remove from queue" onclick={() => unqueue(row, true)}><X size={12} /></button>
              </li>
            {/each}
          </ul>
        {/if}

        <button
          class="row"
          disabled={plan.moves.length === 0}
          onclick={() => (expanded = expanded === "moves" ? null : "moves")}
        >
          <span class="icon"><FolderInput size={16} /></span>
          <span class="what">Move {plan.moves.length} file(s)</span>
        </button>
        {#if expanded === "moves"}
          <ul class="detail">
            {#each moveRows as row}
              <li>
                {row.label} → {row.dest}/
                <button class="unqueue" title="Remove from queue" onclick={() => unqueue(row, false)}><X size={12} /></button>
              </li>
            {/each}
          </ul>
        {/if}

        <button
          class="row"
          disabled={plan.copies.length === 0}
          onclick={() => (expanded = expanded === "copies" ? null : "copies")}
        >
          <span class="icon"><Copy size={16} /></span>
          <span class="what">Copy {plan.copies.length} file(s)</span>
        </button>
        {#if expanded === "copies"}
          <ul class="detail">
            {#each copyRows as row}
              <li>
                {row.label} → {row.dest}/
                <button class="unqueue" title="Remove from queue" onclick={() => unqueue(row, false)}><X size={12} /></button>
              </li>
            {/each}
          </ul>
        {/if}

        <div class="row static">
          <span class="icon"><Tag size={16} /></span>
          <span class="what">Write {plan.xmpCount} XMP sidecar(s)</span>
          <span class="how">rating · flag · color label</span>
        </div>
      </div>

      <div class="mode">
        <label for="delmode">Deletion mode</label>
        <select id="delmode" value={plan.deletionMode} onchange={changeMode} disabled={running}>
          <option value="trash">Project _trash folder</option>
          <option value="permanent">Permanent (no undo!)</option>
        </select>
      </div>

      {#if running}
        <progress max={progress.total || 1} value={progress.done}></progress>
      {/if}

      {#if outcome}
        <p class="outcome" class:bad={outcome.errors > 0}>
          Done: {outcome.ok} ok{outcome.errors > 0 ? `, ${outcome.errors} failed` : ""}.
          {#each outcome.errorSamples as s}<br />· {s}{/each}
        </p>
      {/if}

      {#if error}
        <p class="error">{error}</p>
      {/if}

      <footer>
        <span class="summary">{total} operation(s)</span>
        <button class="primary" disabled={running || total === 0} onclick={execute}>
          {#if plan.deletionMode === "permanent" && plan.deletes.length > 0}
            <TriangleAlert size={14} style="vertical-align: -2px" />
            Execute (deletes {plan.deletes.length} files permanently)
          {:else}
            Execute
          {/if}
        </button>
      </footer>
    {:else}
      <p>Loading…</p>
      {#if error}<p class="error">{error}</p>{/if}
    {/if}
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
    /* Guaranteed gutter on every side: the larger of the safe-area inset (system
       bars / cutout) and the shared edge margin, so the panel never touches the
       screen edges on mobile. The dialog then fills the padded area. */
    padding: max(var(--inset-top), var(--dialog-edge-margin))
      max(var(--inset-right), var(--dialog-edge-margin))
      max(var(--inset-bottom), var(--dialog-edge-margin))
      max(var(--inset-left), var(--dialog-edge-margin));
    box-sizing: border-box;
  }

  .dialog {
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    padding: 16px 20px;
    width: 520px;
    max-width: 100%;
    max-height: 100%;
    display: flex;
    flex-direction: column;
    gap: 10px;
    overflow-y: auto;
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

  .conflicts {
    background: #3a2e1e;
    border: 1px solid #7a5c2e;
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
  }

  .conflicts ul {
    margin: 4px 0 0;
    padding-left: 18px;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    text-align: left;
    padding: 8px 10px;
    background: var(--control);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: #e8e8e8;
    font-size: 13px;
    font-family: inherit;
    cursor: pointer;
  }

  .row.static {
    cursor: default;
  }

  .row:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .icon {
    display: inline-flex;
    align-items: center;
    opacity: 0.85;
  }

  .what {
    flex: 1;
  }

  .how {
    opacity: 0.55;
    font-size: 12px;
  }

  .detail {
    margin: 0;
    padding: 4px 12px 8px 34px;
    font-size: 12px;
    opacity: 0.85;
    max-height: 180px;
    overflow-y: auto;
  }

  .detail li {
    display: flex;
    gap: 8px;
    align-items: center;
  }

  .unqueue {
    display: inline-flex;
    align-items: center;
    background: none;
    border: none;
    color: #888;
    cursor: pointer;
    font-size: 11px;
    padding: 0 4px;
  }

  .unqueue:hover {
    color: #ff6b6b;
  }

  .mode {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
  }

  select,
  button {
    border-radius: 6px;
    border: 1px solid var(--border-strong);
    padding: 5px 10px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: var(--control);
    cursor: pointer;
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
    opacity: 1;
  }

  footer {
    display: flex;
    align-items: center;
    gap: 10px;
    border-top: 1px solid var(--border);
    padding-top: 10px;
  }

  .summary {
    flex: 1;
    font-size: 12px;
    opacity: 0.6;
  }

  .primary {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    border-color: var(--accent);
    font-weight: 600;
  }

  .primary:disabled {
    opacity: 0.45;
    cursor: default;
  }

  progress {
    width: 100%;
  }

  .outcome {
    font-size: 13px;
    color: #6be675;
    margin: 0;
  }

  .outcome.bad {
    color: #ffb86b;
  }

  .error {
    color: #ff6b6b;
    font-size: 13px;
    margin: 0;
  }
</style>
