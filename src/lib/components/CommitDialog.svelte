<script lang="ts">
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { onMount, tick } from "svelte";
  import {
    api,
    type CommitEntry,
    type CommitOutcome,
    type CommitPlan,
    type CommitSection,
    type CommitSummary,
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
  import Check from "@lucide/svelte/icons/check";
  import LoaderCircle from "@lucide/svelte/icons/loader-circle";
  import History from "@lucide/svelte/icons/history";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import X from "@lucide/svelte/icons/x";
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";

  let plan = $state<CommitPlan | null>(null);
  let running = $state(false);
  let error = $state("");
  let expanded = $state<CommitSection | null>(null);
  let refreshRequest = 0;
  let alive = true;

  function isDialogCurrent(generation: number): boolean {
    return alive && session.commitDialogOpen && generation === catalog.generation;
  }

  // Per-phase commit progress. `runningPhases` are the sections a single
  // execution touches (all four for Execute, one for a hold-to-run button);
  // `activePhase` + `phaseDone`/`phaseTotal` track the step the backend is on,
  // and `completedPhases` accumulates the finished ones — together they drive
  // the "Committing changes…" checklist.
  let runningPhases = $state<CommitSection[]>([]);
  let activePhase = $state<CommitSection | null>(null);
  let phaseDone = $state(0);
  let phaseTotal = $state(0);
  let completedPhases = $state<Set<CommitSection>>(new Set());

  const PHASE_LABEL: Record<CommitSection, string> = {
    deletes: "Deleting",
    moves: "Moving",
    copies: "Copying",
    xmp: "Writing XMP",
  };

  const deletionModeNames: Record<DeletionMode, string> = {
    permanent: "Permanent delete",
    trash: "Project _trash folder",
  };

  async function refresh() {
    const generation = catalog.generation;
    const request = ++refreshRequest;
    error = "";
    try {
      const next = await api.commitPreview();
      if (isDialogCurrent(generation) && request === refreshRequest) plan = next;
    } catch (e) {
      if (isDialogCurrent(generation) && request === refreshRequest) error = String(e);
    }
  }

  async function changeMode(e: Event) {
    const mode = (e.target as HTMLSelectElement).value;
    try {
      await api.setProjectSetting("deletionMode", mode);
      await refresh();
    } catch (err) {
      // Re-read the persisted mode (unchanged on failure) so the select rolls
      // back, then surface the error — refresh() clears it, so set it after.
      await refresh();
      error = String(err);
    }
  }

  function sectionCount(p: CommitPlan, s: CommitSection): number {
    switch (s) {
      case "deletes":
        return p.deletes.length;
      case "moves":
        return p.moves.length;
      case "copies":
        return p.copies.length;
      case "xmp":
        return p.xmpCount;
    }
  }

  function sectionHash(p: CommitPlan, s: CommitSection): string {
    switch (s) {
      case "deletes":
        return p.deletesHash;
      case "moves":
        return p.movesHash;
      case "copies":
        return p.copiesHash;
      case "xmp":
        return p.xmpHash;
    }
  }

  function activePhasesOf(p: CommitPlan): CommitSection[] {
    return (["deletes", "moves", "copies", "xmp"] as CommitSection[]).filter(
      (s) => sectionCount(p, s) > 0,
    );
  }

  function startRun(phases: CommitSection[]) {
    running = true;
    runningPhases = phases;
    // Seed the first step from the plan we already hold, so the panel opens on a
    // real named phase with a real total instead of an empty checklist waiting
    // for the backend's first event.
    activePhase = phases[0] ?? null;
    phaseDone = 0;
    phaseTotal = plan && phases[0] ? sectionCount(plan, phases[0]) : 0;
    completedPhases = new Set();
    error = "";
  }

  // Resolves once the browser has drawn the current DOM. requestAnimationFrame
  // runs just BEFORE the paint, so we hop one more task to land after it. A
  // background window never fires rAF, so a fallback keeps the commit moving.
  function afterPaint(): Promise<void> {
    return new Promise((resolve) => {
      requestAnimationFrame(() => setTimeout(resolve));
      setTimeout(resolve, 100);
    });
  }

  // Shared execute path for both the whole-plan Execute and the per-section
  // hold-to-run buttons: run, then hand the result to a popup and close this
  // dialog just before it shows. On failure the dialog stays open with the error.
  async function runCommit(phases: CommitSection[], run: () => Promise<CommitOutcome>) {
    const generation = catalog.generation;
    if (phases.some((phase) => phase !== "xmp")) session.invalidateRejectionUndo();
    startRun(phases);
    // The commit command is synchronous on the backend, so it stalls the app for
    // its whole duration. Get the busy panel committed to the DOM and drawn on
    // screen BEFORE the call goes out, or the user watches the old view freeze
    // and only sees the progress at the very end. This matters most for the
    // per-section run, which fires from inside a requestAnimationFrame callback:
    // without the yield its state change and the blocking call land in the same
    // frame, so that frame never reaches the screen.
    await tick();
    await afterPaint();
    if (generation !== catalog.generation || !session.commitDialogOpen) {
      running = false;
      return;
    }
    try {
      const oc = await run();
      if (generation !== catalog.generation) return;
      await catalog.refresh();
      if (generation !== catalog.generation) return;
      await session.refreshPending();
      if (generation !== catalog.generation) return;
      const samples = oc.errorSamples.map((s) => `· ${s}`).join("\n");
      session.commitDone = {
        title: oc.errors > 0 ? "Commit finished with errors" : "Commit complete",
        message:
          `Done: ${oc.ok} ok${oc.errors > 0 ? `, ${oc.errors} failed` : ""}.` +
          (samples ? `\n${samples}` : ""),
      };
      close();
    } catch (e) {
      if (generation !== catalog.generation) return;
      await refresh();
      error = String(e);
    } finally {
      running = false;
      activePhase = null;
    }
  }

  async function executeAll() {
    if (!plan) return;
    const hash = plan.planHash;
    await runCommit(activePhasesOf(plan), () => api.commitExecute(hash));
  }

  async function executeSection(section: CommitSection) {
    if (!plan || running || !session.commitDialogOpen) return;
    const hash = sectionHash(plan, section);
    await runCommit([section], () => api.commitExecuteSection(section, hash));
  }

  // --- hold-to-run: press and hold a section row; its background fills until it
  // reaches the end, then that section commits. A quick tap instead toggles the
  // detail list. Permanent deletes fill red and take longer — the gesture is its
  // own confirmation for an irreversible action.
  const HOLD_MS = 1500;
  const HOLD_MS_PERMANENT = 3000;
  // Grace period before the fill even starts: a quick tap (which toggles the
  // detail list) must not flash the blue fill. Releasing within this window is
  // treated as a tap, not a hold, so the two gestures never overlap.
  const HOLD_DELAY_MS = 250;

  // Keyed by an arbitrary string so the same gesture drives both the commit
  // sections ("section:deletes") and the history rows ("commit:12") — one
  // interaction language for "this is destructive, mean it".
  let holdKey = $state<string | null>(null);
  let holdFrac = $state(0);
  let holdRaf = 0;
  let holdStart = 0;
  let holdFired = false;

  function isDangerHold(section: CommitSection): boolean {
    return section === "deletes" && plan?.deletionMode === "permanent";
  }

  function holdDuration(section: CommitSection): number {
    return isDangerHold(section) ? HOLD_MS_PERMANENT : HOLD_MS;
  }

  function resetHold() {
    if (holdRaf) cancelAnimationFrame(holdRaf);
    holdRaf = 0;
    holdKey = null;
    holdFrac = 0;
  }

  /** Begin a press-and-hold on `key`; `onFire` runs if it reaches the end. */
  function startHold(key: string, durationMs: number, e: PointerEvent | KeyboardEvent, onFire: () => void) {
    if ("button" in e && e.button !== 0) return;
    resetHold();
    e.preventDefault();
    if ("pointerId" in e) (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    holdKey = key;
    holdFrac = 0;
    holdFired = false;
    holdStart = performance.now();
    const step = () => {
      if (holdKey !== key || !session.commitDialogOpen) return;
      // The fill only advances AFTER the grace period, so the first HOLD_DELAY_MS
      // of a press show nothing (that window belongs to the tap-to-expand gesture).
      const held = performance.now() - holdStart - HOLD_DELAY_MS;
      holdFrac = Math.min(1, Math.max(0, held) / durationMs);
      if (holdFrac >= 1) {
        holdFired = true;
        resetHold();
        onFire();
        return;
      }
      holdRaf = requestAnimationFrame(step);
    };
    holdRaf = requestAnimationFrame(step);
  }

  /** End a press on `key`; `onTap` runs when it was a tap, not a hold. */
  function endHold(key: string, onTap?: () => void) {
    if (holdKey !== key) return;
    const elapsed = performance.now() - holdStart;
    const fired = holdFired;
    resetHold();
    // A release within the grace period is a tap. Past it, the hold simply
    // didn't complete — do nothing.
    if (!fired && elapsed < HOLD_DELAY_MS) onTap?.();
  }

  function fillOf(key: string): number {
    return holdKey === key ? holdFrac * 100 : 0;
  }

  function onRowKeydown(section: CommitSection, e: KeyboardEvent) {
    if (e.key !== "Enter" && e.key !== " ") return;
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    if (e.key === "Enter") expanded = expanded === section ? null : section;
    else startHold(`section:${section}`, holdDuration(section), e, () => void executeSection(section));
  }

  function onRowKeyup(section: CommitSection, e: KeyboardEvent) {
    if (e.key !== " ") return;
    e.preventDefault();
    endHold(`section:${section}`);
  }

  // Pending stays the default view, so the commit flow is unchanged; History is
  // here rather than behind its own toolbar button because it is the same
  // subject seen from the other side.
  type Tab = "pending" | "history";
  let tab = $state<Tab>("pending");
  let commits = $state<CommitSummary[] | null>(null);
  let openCommit = $state<number | null>(null);
  let entries = $state<CommitEntry[]>([]);
  let undoing = $state(false);
  let historyRequest = 0;
  let detailRequest = 0;
  interface UndoCounts {
    filesUndone: number;
    filesRetained: number;
    sidecarsRetained: number;
  }
  let undoCounts = $state<Record<number, UndoCounts>>({});

  function countUndo(entries: CommitEntry[]): UndoCounts {
    const successful = entries.filter((entry) => entry.result === 0);
    return {
      filesUndone: successful.filter((entry) => entry.action !== 3 && entry.undoneAt !== null).length,
      filesRetained: successful.filter((entry) => entry.action !== 3 && entry.undoneAt === null).length,
      sidecarsRetained: successful.filter((entry) => entry.action === 3 && entry.undoneAt === null).length,
    };
  }

  const ACTION_NAMES: Record<number, string> = {
    0: "Deleted",
    1: "Moved",
    2: "Copied",
    3: "Wrote sidecar",
  };

  async function refreshHistory() {
    const generation = catalog.generation;
    const request = ++historyRequest;
    try {
      const next = await api.listCommits();
      const details = await Promise.all(next.filter((commit) => commit.undoneAt !== null ||
        commit.undoable < commit.deletes + commit.moves + commit.copies)
        .map(async (commit) => [commit.id, countUndo(await api.commitDetail(commit.id))] as const));
      if (isDialogCurrent(generation) && request === historyRequest) {
        undoCounts = Object.fromEntries(details.filter(([, counts]) => counts.filesUndone > 0));
        commits = next;
      }
    } catch (e) {
      if (isDialogCurrent(generation) && request === historyRequest) error = String(e);
    }
  }

  async function showHistory() {
    tab = "history";
    error = "";
    await refreshHistory();
  }

  async function refreshCommitEntries(id: number) {
    const generation = catalog.generation;
    const request = ++detailRequest;
    const isCurrent = () => isDialogCurrent(generation) && request === detailRequest && openCommit === id;
    entries = [];
    try {
      const next = await api.commitDetail(id);
      if (isCurrent()) entries = next;
    } catch (e) {
      if (isCurrent()) error = String(e);
    }
  }

  async function toggleCommit(id: number) {
    if (openCommit === id) {
      ++detailRequest;
      openCommit = null;
      entries = [];
      return;
    }
    openCommit = id;
    await refreshCommitEntries(id);
  }

  /** Shared tail for both undo paths: report, then resync grid and queue. */
  async function runUndo(run: () => Promise<void>) {
    const generation = catalog.generation;
    session.invalidateRejectionUndo();
    undoing = true;
    error = "";
    try {
      await run();
      if (!isDialogCurrent(generation)) return;
      await catalog.refresh();
      if (!isDialogCurrent(generation)) return;
      await session.refreshPending();
      if (!isDialogCurrent(generation)) return;
      await refreshHistory();
      if (!isDialogCurrent(generation)) return;
      if (openCommit !== null) await refreshCommitEntries(openCommit);
    } catch (e) {
      if (isDialogCurrent(generation)) error = String(e);
    } finally {
      undoing = false;
    }
  }

  async function undoWholeCommit(id: number) {
    await runUndo(async () => {
      const oc = await api.undoCommit(id);
      // Success needs no popup: the row restyles itself to "Undone" and the
      // grid behind has already refreshed. Only failures get interrupted for.
      if (oc.errors > 0) {
        const samples = oc.errorSamples.map((s) => `· ${s}`).join("\n");
        session.commitDone = {
          title: "Undo finished with errors",
          message:
            `Restored ${oc.restored}, ${oc.errors} failed.` + (samples ? `\n${samples}` : ""),
        };
      }
    });
  }

  async function undoOneEntry(entryId: number) {
    await runUndo(async () => {
      await api.undoCommitEntry(entryId);
    });
  }

  function whenOf(secs: number): string {
    return new Date(secs * 1000).toLocaleString();
  }

  function undoSummary(counts: UndoCounts): string {
    const parts = [`${counts.filesUndone} file operation${counts.filesUndone === 1 ? "" : "s"} undone`];
    if (counts.filesRetained) parts.push(`${counts.filesRetained} file operation${counts.filesRetained === 1 ? "" : "s"} retained`);
    if (counts.sidecarsRetained) parts.push(`${counts.sidecarsRetained} sidecar${counts.sidecarsRetained === 1 ? "" : "s"} retained`);
    return parts.join(" · ");
  }

  /** "3 deleted · 2 moved" — only the parts that happened. */
  function summaryOf(c: CommitSummary): string {
    const undone = undoCounts[c.id];
    if (undone) return undoSummary(undone);
    const parts: string[] = [];
    if (c.deletes) parts.push(`${c.deletes} deleted`);
    if (c.moves) parts.push(`${c.moves} moved`);
    if (c.copies) parts.push(`${c.copies} copied`);
    if (c.xmp) parts.push(`${c.xmp} sidecar${c.xmp === 1 ? "" : "s"}`);
    return parts.join(" · ") || "nothing";
  }

  function onRowPointerDown(section: CommitSection, e: PointerEvent) {
    if (!plan || running || sectionCount(plan, section) === 0) return;
    startHold(`section:${section}`, holdDuration(section), e, () => void executeSection(section));
  }

  function onRowPointerUp(section: CommitSection) {
    // XMP has no detail list, so its tap is inert.
    endHold(`section:${section}`, () => {
      if (section !== "xmp") expanded = expanded === section ? null : section;
    });
  }

  async function unqueue(row: PlanRow, isDelete: boolean) {
    if (isDelete) session.invalidateRejectionUndo();
    try {
      await api.removePending(row.ids);
      // Deletes mirror the reject flag: unqueueing a delete also un-rejects the
      // files, keeping flag and queue in sync in both directions. Rows are already
      // per-file expanded, hence asGroups: false. Moves/copies leave flags alone.
      if (isDelete) await api.setFlag({ ids: row.fileIds, asGroups: false }, 0);
      await session.refreshPending();
      await refresh();
    } catch (e) {
      // Surface the failure so a flag/queue invariant divergence is visible
      // (refresh() clears error, so set it after re-reading the plan). Re-read
      // pending too, so the grid badge reflects any partial removePending.
      await session.refreshPending();
      await refresh();
      error = String(e);
    }
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

  // Extension (no dot, lowercased) of a relPath — used only for the collapsed
  // pair's display label, never for pairing identity.
  function extOf(relPath: string): string {
    const dot = relPath.lastIndexOf(".");
    return dot >= 0 ? relPath.slice(dot + 1).toLowerCase() : "";
  }

  function isJpegExt(ext: string): boolean {
    return ext === "jpg" || ext === "jpeg";
  }

  // relPath with its extension removed, so the two halves of a RAW+JPEG pair
  // ("folder/IMG.CR3", "folder/IMG.JPG") share one stem ("folder/IMG").
  function stemOf(relPath: string): string {
    const dot = relPath.lastIndexOf(".");
    return dot >= 0 ? relPath.slice(0, dot) : relPath;
  }

  function pairedPlanRow(members: PendingAction[]): PlanRow | null {
    if (members.length !== 2) return null;
    const jpeg = members.find((m) => isJpegExt(extOf(m.relPath)));
    const raw = members.find((m) => !isJpegExt(extOf(m.relPath)));
    if (!jpeg || !raw) return null;
    return {
      label: `${raw.relPath}+${extOf(jpeg.relPath)}`,
      ids: [raw.id, jpeg.id],
      fileIds: [raw.fileId, jpeg.fileId],
      dest: raw.dest,
    };
  }

  // Collapse a RAW+JPEG pair queued for the SAME action into one line
  // ("AJDJ8378.CR3+jpg"). A shared pairToken is necessary but not sufficient:
  // the backend also stamps one token across a whole multi-file enqueue batch,
  // so two unrelated files rejected together share a token. We therefore group
  // only two members that share the token AND the same stem (a real pair differs
  // only in extension). A null token, or a batch of more than two, stays split.
  // Each pair is emitted at the position of its first member, preserving order.
  function planRows(items: PendingAction[]): PlanRow[] {
    const groups = new Map<string, PendingAction[]>();
    for (const p of items) {
      if (p.pairToken == null) continue;
      const key = `${p.pairToken} ${stemOf(p.relPath)}`;
      const members = groups.get(key) ?? [];
      members.push(p);
      groups.set(key, members);
    }
    const rows: PlanRow[] = [];
    const collapsed = new Set<string>();
    for (const p of items) {
      if (p.pairToken != null) {
        const key = `${p.pairToken} ${stemOf(p.relPath)}`;
        if (collapsed.has(key)) continue;
        const pair = pairedPlanRow(groups.get(key) ?? []);
        if (pair) {
          rows.push(pair);
          collapsed.add(key);
          continue;
        }
      }
      rows.push({ label: p.relPath, ids: [p.id], fileIds: [p.fileId], dest: p.dest });
    }
    return rows;
  }

  function close() {
    resetHold();
    ++refreshRequest;
    ++historyRequest;
    ++detailRequest;
    session.commitDialogOpen = false;
  }

  const dismiss = backdropDismiss(close);

  function onKeydown(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") close();
  }

  onMount(() => {
    refresh();
    let unlisten: UnlistenFn | undefined;
    // The dialog can unmount before listen() resolves; track that so the
    // subscription is torn down immediately once it does, instead of leaking.
    let cancelled = false;
    listen<{ projectRoot: string; phase: CommitSection; done: number; total: number }>("commit:progress", (e) => {
      if (!catalog.acceptEvent(e.payload)) return;
      const { phase, done, total } = e.payload;
      // A phase change means the previous one finished — bank it for the checklist.
      if (activePhase && activePhase !== phase && !completedPhases.has(activePhase)) {
        completedPhases = new Set(completedPhases).add(activePhase);
      }
      activePhase = phase;
      phaseDone = done;
      phaseTotal = total;
    }).then((u) => {
      unlisten = u;
      if (cancelled) unlisten();
    });
    return () => {
      cancelled = true;
      alive = false;
      resetHold();
      unlisten?.();
    };
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
  {...dismiss}
  onkeydown={(e) => e.key === "Escape" && close()}
  role="presentation"
>
  <div
    class="dialog"
    use:modalFocus
    onclick={(e) => e.stopPropagation()}
    onkeydown={onKeydown}
    role="dialog"
    aria-labelledby="review-changes-heading"
    tabindex="-1"
  >
    <header>
      <h2 id="review-changes-heading">{tab === "pending" ? "Review changes" : "Change history"}</h2>
      <button class="close-x" onclick={close} aria-label="Close" title="Close" disabled={running}>
        <X size={18} />
      </button>
    </header>

    <div class="tabs">
      <button class="tab" class:on={tab === "pending"} disabled={running} onclick={() => (tab = "pending")}>
        Pending
      </button>
      <button class="tab" class:on={tab === "history"} disabled={running} onclick={() => void showHistory()}>
        <History size={13} /> History
      </button>
    </div>

    {#if tab === "history"}
      {#if commits === null}
        <p class="loading">Loading…</p>
      {:else if commits.length === 0}
        <p class="loading">Nothing committed in this project yet.</p>
      {:else}
        <div class="history">
          {#each commits as c (c.id)}
            <div class="hcommit" class:undone={c.undoneAt !== null}>
              <!-- A commit with something to undo gets the hold gesture, and
                   its tap is handled on release; one with nothing left to undo
                   never arms a hold, so a plain click expands it. -->
              <button
                class="hrow"
                onpointerdown={(e) =>
                  c.undoable > 0 && !undoing
                    ? startHold(`commit:${c.id}`, HOLD_MS, e, () => void undoWholeCommit(c.id))
                    : undefined}
                onpointerup={() => endHold(`commit:${c.id}`, () => void toggleCommit(c.id))}
                onpointercancel={resetHold}
                onblur={resetHold}
                onclick={(e) => (e.detail === 0 || c.undoable === 0 ? void toggleCommit(c.id) : undefined)}
                aria-expanded={openCommit === c.id}
                aria-controls={`commit-entries-${c.id}`}
                title={c.undoable > 0 ? "Tap to expand · hold to undo" : "Tap to expand"}
              >
                <span class="hold-fill" style="width: {fillOf(`commit:${c.id}`)}%"></span>
                <span class="hwhen">{whenOf(c.startedAt)}</span>
                <span class="hwhat" title={summaryOf(c)}>{summaryOf(c)}</span>
                {#if c.errors > 0}
                  <span class="hbadge err"><TriangleAlert size={11} /> {c.errors}</span>
                {/if}
                {#if c.undoneAt !== null || undoCounts[c.id]}
                  <span class="hbadge done">{undoCounts[c.id]?.filesRetained || undoCounts[c.id]?.sidecarsRetained ? "Partial undo" : "Undone"}</span>
                {:else if c.undoable > 0}
                  <span class="hbadge"><Undo2 size={11} /> {c.undoable}</span>
                {/if}
              </button>
              {#if c.undoable > 0}
                <button class="hundo" disabled={undoing} onclick={() => void undoWholeCommit(c.id)}>
                  <Undo2 size={13} /> Undo reversible actions
                </button>
              {/if}

              {#if openCommit === c.id}
                <ul class="hentries" id={`commit-entries-${c.id}`}>
                  {#each entries as e (e.id)}
                    <li class:gone={e.undoneAt !== null}>
                      <span class="eaction">{ACTION_NAMES[e.action] ?? "?"}</span>
                      <span class="epath" title={e.beforePath ?? ""}>{e.beforePath ?? "—"}</span>
                      {#if e.afterPath && e.action !== 0}
                        <span class="edest">→ {e.afterPath}</span>
                      {/if}
                      {#if e.undoable}
                        <button
                          class="eundo"
                          disabled={undoing}
                          title="Undo just this one"
                          aria-label={`Undo ${e.beforePath ?? "entry"}`}
                          onclick={() => void undoOneEntry(e.id)}
                        >
                          <Undo2 size={12} />
                        </button>
                      {:else}
                        <!-- Say why, rather than showing a dead button. -->
                        <span class="ewhy">{e.error ?? e.blockedReason ?? ""}</span>
                      {/if}
                    </li>
                  {/each}
                </ul>
              {/if}
            </div>
          {/each}
        </div>
        <p class="hint">Activate a commit to see its actions. Use Undo reversible actions to restore its files.</p>
      {/if}
      {#if error}
        <p class="error">{error}</p>
      {/if}
    {:else if plan}
      {#if running}
        <div class="committing">
          <p class="committing-title">
            <span class="spin"><LoaderCircle size={16} /></span> Committing changes…
          </p>
          <ul class="phases">
            {#each runningPhases as ph (ph)}
              <li class="phase" class:done={completedPhases.has(ph)} class:active={activePhase === ph}>
                <span class="phase-icon">
                  {#if completedPhases.has(ph)}
                    <Check size={14} />
                  {:else if activePhase === ph}
                    <span class="spin"><LoaderCircle size={14} /></span>
                  {:else}
                    <span class="pending-dot"></span>
                  {/if}
                </span>
                <span class="phase-name">{PHASE_LABEL[ph]}</span>
                {#if activePhase === ph}
                  <span class="phase-count">{phaseDone}/{phaseTotal}</span>
                {:else if completedPhases.has(ph)}
                  <span class="phase-count">done</span>
                {/if}
              </li>
            {/each}
          </ul>
          {#if activePhase}
            <progress max={phaseTotal || 1} value={phaseDone}></progress>
          {/if}
        </div>
      {:else}
        {#if plan.conflicts.length > 0}
          <div class="conflicts">
            <TriangleAlert size={14} style="vertical-align: -2px" />
            {plan.conflicts.length} {plan.conflicts.length === 1 ? "conflict" : "conflicts"}:
            <ul>
              {#each plan.conflicts.slice(0, 5) as c}<li>{c}</li>{/each}
            </ul>
          </div>
        {/if}

        <p class="hint">
          <span class="fine">Click a row to expand · hold it to run that section on its own.</span>
          <span class="coarse">Tap a row to expand · hold it to run that section on its own.</span>
          Enter expands the focused row. Hold Space to run that section.
        </p>

        <div class="rows">
          <button
            class="row"
            disabled={plan.deletes.length === 0}
            onpointerdown={(e) => onRowPointerDown("deletes", e)}
            onpointerup={() => onRowPointerUp("deletes")}
            onpointercancel={resetHold}
            onkeydown={(e) => onRowKeydown("deletes", e)}
            onkeyup={(e) => onRowKeyup("deletes", e)}
            onblur={resetHold}
            title="Tap to expand · hold to delete"
          >
            <span
              class="hold-fill"
              class:danger={isDangerHold("deletes")}
              style="width: {fillOf('section:deletes')}%"
            ></span>
            <span class="icon"><Trash2 size={16} /></span>
            <span class="what">Delete {plan.deletes.length} {plan.deletes.length === 1 ? "file" : "files"}</span>
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
            onpointerdown={(e) => onRowPointerDown("moves", e)}
            onpointerup={() => onRowPointerUp("moves")}
            onpointercancel={resetHold}
            onkeydown={(e) => onRowKeydown("moves", e)}
            onkeyup={(e) => onRowKeyup("moves", e)}
            onblur={resetHold}
            title="Tap to expand · hold to move"
          >
            <span class="hold-fill" style="width: {fillOf('section:moves')}%"></span>
            <span class="icon"><FolderInput size={16} /></span>
            <span class="what">Move {plan.moves.length} {plan.moves.length === 1 ? "file" : "files"}</span>
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
            onpointerdown={(e) => onRowPointerDown("copies", e)}
            onpointerup={() => onRowPointerUp("copies")}
            onpointercancel={resetHold}
            onkeydown={(e) => onRowKeydown("copies", e)}
            onkeyup={(e) => onRowKeyup("copies", e)}
            onblur={resetHold}
            title="Tap to expand · hold to copy"
          >
            <span class="hold-fill" style="width: {fillOf('section:copies')}%"></span>
            <span class="icon"><Copy size={16} /></span>
            <span class="what">Copy {plan.copies.length} {plan.copies.length === 1 ? "file" : "files"}</span>
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

          <button
            class="row"
            disabled={plan.xmpCount === 0}
            onpointerdown={(e) => onRowPointerDown("xmp", e)}
            onpointerup={() => onRowPointerUp("xmp")}
            onpointercancel={resetHold}
            onkeydown={(e) => onRowKeydown("xmp", e)}
            onkeyup={(e) => onRowKeyup("xmp", e)}
            onblur={resetHold}
            title="Hold to write sidecars"
          >
            <span class="hold-fill" style="width: {fillOf('section:xmp')}%"></span>
            <span class="icon"><Tag size={16} /></span>
            <span class="what">Write {plan.xmpCount} XMP {plan.xmpCount === 1 ? "sidecar" : "sidecars"}</span>
            <span class="how">rating · flag · color label</span>
          </button>
        </div>

        <div class="mode">
          <label for="delmode">Deletion mode</label>
          <select id="delmode" value={plan.deletionMode} onchange={changeMode}>
            <option value="trash">Project _trash folder</option>
            <option value="permanent">Permanent (no undo!)</option>
          </select>
        </div>

        {#if error}
          <p class="error">{error}</p>
        {/if}

        <footer>
          <span class="summary">{total} {total === 1 ? "operation" : "operations"}</span>
          <button class="primary" disabled={total === 0} onclick={executeAll}>
            {#if plan.deletionMode === "permanent" && plan.deletes.length > 0}
              <TriangleAlert size={14} style="vertical-align: -2px" />
              Execute (deletes {plan.deletes.length} {plan.deletes.length === 1 ? "file" : "files"} permanently)
            {:else}
              Execute
            {/if}
          </button>
        </footer>
      {/if}
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
    outline: none;
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

  .hint {
    margin: 0;
    font-size: 11px;
    opacity: 0.5;
  }

  /* One hint, worded for the pointer actually in use: "tap" is wrong with a
     mouse, and "click" is wrong on a phone. */
  .hint .coarse {
    display: none;
  }

  @media (pointer: coarse) {
    .hint .fine {
      display: none;
    }
    .hint .coarse {
      display: inline;
    }
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .row {
    /* `position: relative` + `overflow: hidden` clip the hold-fill to the row's
       rounded rectangle; its contents sit above the fill via z-index. */
    position: relative;
    overflow: hidden;
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
    /* A long press must not select the label text or fire the OS callout. */
    user-select: none;
    touch-action: none;
  }

  .row:disabled {
    opacity: 0.4;
    cursor: default;
  }

  .hold-fill {
    position: absolute;
    left: 0;
    top: 0;
    bottom: 0;
    width: 0;
    background: var(--accent-fill);
    opacity: 0.55;
    pointer-events: none;
    z-index: 0;
  }

  .hold-fill.danger {
    background: #b3352e;
    opacity: 0.6;
  }

  .row > .icon,
  .row > .what,
  .row > .how {
    position: relative;
    z-index: 1;
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

  select {
    padding-right: 28px;
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

  .close-x:disabled {
    opacity: 0.3;
    cursor: default;
  }

  .committing {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 4px 0;
  }

  .committing-title {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }

  .phases {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .phase {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13px;
    opacity: 0.5;
  }

  .phase.active,
  .phase.done {
    opacity: 1;
  }

  .phase-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    color: var(--accent);
  }

  .phase.done .phase-icon {
    color: #6be675;
  }

  .pending-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    opacity: 0.6;
  }

  .phase-name {
    flex: 1;
  }

  .phase-count {
    font-size: 12px;
    opacity: 0.6;
    font-variant-numeric: tabular-nums;
  }

  .spin {
    display: inline-flex;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
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

  .error {
    color: #ff6b6b;
    font-size: 13px;
    margin: 0;
  }


  .tabs {
    display: flex;
    gap: 4px;
  }

  .tab {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--control);
    border: 1px solid transparent;
    border-radius: 3px;
    color: #bbb;
    padding: 4px 10px;
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .tab:hover:not(.on):not(:disabled) {
    border-color: var(--accent);
  }

  .tab.on {
    background: var(--accent-fill);
    color: #fff;
  }

  .tab:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .loading {
    margin: 0;
    font-size: 13px;
    opacity: 0.6;
  }

  .history {
    display: flex;
    flex-direction: column;
    gap: 4px;
    overflow-y: auto;
  }

  .hrow {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    overflow: hidden;
    background: var(--control);
    border: 1px solid transparent;
    border-radius: 4px;
    color: #ddd;
    padding: 7px 10px;
    cursor: pointer;
    font-family: inherit;
    font-size: 12px;
    text-align: left;
  }

  .hrow:hover {
    border-color: var(--accent);
  }

  .hundo {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin: 4px 0 8px;
  }

  .hcommit.undone .hrow {
    opacity: 0.55;
  }

  .hwhen,
  .hwhat,
  .hbadge {
    position: relative;
    z-index: 1;
  }

  .hwhen {
    flex: none;
    font-variant-numeric: tabular-nums;
    opacity: 0.75;
  }

  .hwhat {
    flex: 1;
    min-width: 0;
    white-space: normal;
    overflow-wrap: anywhere;
  }

  .hbadge {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex: none;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    padding: 1px 7px;
    font-size: 11px;
    opacity: 0.85;
  }

  .hbadge.err {
    color: #ff8f8f;
    border-color: #6b3030;
  }

  .hbadge.done {
    opacity: 0.6;
  }

  .hentries {
    list-style: none;
    margin: 2px 0 6px;
    padding: 0 0 0 10px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .hentries li {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
    color: #bbb;
    padding: 2px 0;
  }

  .hentries li.gone {
    opacity: 0.45;
    text-decoration: line-through;
  }

  .eaction {
    flex: none;
    opacity: 0.65;
    min-width: 84px;
  }

  .epath {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .edest {
    flex: none;
    opacity: 0.6;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .ewhy {
    flex: none;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.5;
    font-style: italic;
  }

  .eundo {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 20px;
    background: none;
    border: 1px solid var(--border-strong);
    border-radius: 4px;
    color: #bbb;
    cursor: pointer;
    padding: 0;
  }

  .eundo:hover:not(:disabled) {
    color: #fff;
    border-color: var(--accent);
  }

  .eundo:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
