<script lang="ts">
  import { previewUrl } from "../api";
  import { session } from "../stores/session.svelte";
  import { view } from "../stores/view.svelte";
  import { keymap } from "../keyboard/dispatcher.svelte";
  import { formatKey, type CommandId } from "../keyboard/keymap";
  import ZoomImage from "./ZoomImage.svelte";
  import VideoPlayer from "./VideoPlayer.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import Check from "@lucide/svelte/icons/check";
  import X from "@lucide/svelte/icons/x";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Search from "@lucide/svelte/icons/search";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Minimize from "@lucide/svelte/icons/minimize";

  const labelColors: Record<string, string> = {
    Red: "#e05555",
    Yellow: "#e0c34f",
    Green: "#59b85e",
    Blue: "#5588e0",
    Purple: "#9a66d6",
  };

  const items = $derived(session.surveyItems);
  const detail = $derived(items.find((item) => item.id === session.focused?.id));
  const review = $derived(session.surveyReview);
  const canReview = $derived(session.surveyKeeperIds.length > 0 && session.surveyKeeperIds.length < items.length);
  const hints = $derived([
    `${binding("nav.prev")} / ${binding("nav.next")} to move`,
    `${binding("flag.reject")} to reject`,
    `${binding("flag.pick")} to pick`,
    `${binding("view.viewer")} to inspect`,
    `${binding("view.back")} to ${view.surveyDetail ? "return" : "leave"}`,
  ].join(" · "));

  function binding(command: CommandId): string {
    const key = [...keymap.bindings].find(([, id]) => id === command)?.[0];
    return key ? formatKey(key) : "Unbound";
  }

  function selectTile(id: number, e: MouseEvent) {
    const index = session.filtered.findIndex((item) => item.id === id);
    if (e.shiftKey) session.rangeSelect(index, e.ctrlKey || e.metaKey);
    else if (e.ctrlKey || e.metaKey) session.toggleSelect(index);
    else session.focusSurveyItem(id);
  }

  // Roughly square: as the field narrows the survivors get a bigger share of
  // the screen, which is the reward for eliminating one.
  const cols = $derived(Math.max(1, Math.ceil(Math.sqrt(items.length))));
</script>

<div class="survey">
  <header>
    <span class="count">
      Batch {session.surveyBatch} · {items.length} {items.length === 1 ? "survivor" : "survivors"}
      <span class="capped">· {session.surveyRemainingCount} awaiting review · {session.surveyRequested} selected</span>
    </span>
    <span class="hint">{hints}</span>
    <button
      data-survey-undo
      disabled={!session.surveyUndo || session.surveyBusy}
      onclick={(e) => { void session.undoSurveyRejection(); e.currentTarget.blur(); }}
      title="Undo last rejection (Ctrl+Z)"
    ><Undo2 size={14} /><span>Undo last rejection</span></button>
    {#if view.surveyDetail}
      <button data-survey-return onclick={(e) => { session.leaveSurveyDetail(); e.currentTarget.blur(); }}>Return to candidates</button>
    {:else}
      <button
        data-survey-inspect
        disabled={!detail || session.surveyBusy}
        onclick={(e) => { session.inspectSurvey(); e.currentTarget.blur(); }}
      ><Search size={14} /><span>Inspect detail</span></button>
      <button
        data-survey-review
        disabled={!canReview || session.surveyBusy}
        onclick={(e) => { session.reviewSurveyKeepers(); e.currentTarget.blur(); }}
        title="Review this batch before you queue any rejection"
      >Keep {session.selectedIds.size > 0 ? "selected" : "picked"} and reject rest…</button>
    {/if}
    {#if session.surveyRemainingCount > 0}
      <button
        data-survey-next-batch
        disabled={session.surveyBusy}
        onclick={(e) => { session.nextSurveyBatch(); e.currentTarget.blur(); }}
      >Next batch ({session.surveyRemainingCount})</button>
    {/if}
    <button
      class="icon"
      onclick={(e) => { view.toggleFullscreen(); e.currentTarget.blur(); }}
      aria-label={view.fullscreen ? "Exit full screen" : "Full screen"}
      title={view.fullscreen ? "Exit full screen" : "Full screen"}
    >{#if view.fullscreen}<Minimize size={15} />{:else}<Maximize size={15} />{/if}</button>
    <button class="close" onclick={() => session.closeSurvey()} aria-label="Close survey" title="Close (Esc)">
      <X size={15} />
    </button>
  </header>

  {#if view.surveyDetail && detail}
    <div class="detail" data-survey-detail>
      {#if detail.kind === 2}
        {#key detail.id}<VideoPlayer item={detail} />{/key}
      {:else}
        <ZoomImage item={detail} />
      {/if}
      <span class="detail-name">{detail.name}</span>
    </div>
  {:else if items.length === 0}
    <div class="empty">
      {session.surveyIds.length === 0 ? "This batch has no candidates. Undo the last rejection or open the next batch." : "No candidates match the current filters."}
    </div>
  {:else}
    <div class="tiles" style="grid-template-columns: repeat({cols}, minmax(0, 1fr))">
    {#each items as item (item.id)}
      <button
        class="tile"
        data-survey-tile={item.id}
        class:focused={session.focused?.id === item.id}
        class:selected={session.selectedIds.has(item.id)}
        aria-pressed={session.selectedIds.has(item.id)}
        onclick={(e) => selectTile(item.id, e)}
        ondblclick={() => { session.focusSurveyItem(item.id); session.inspectSurvey(); }}
      >
        <img src={previewUrl(item)} alt={item.name} draggable="false" />
        <span class="meta">
          {#if item.rating > 0}<span class="stars">{"★".repeat(item.rating)}</span>{/if}
          {#if item.flag === 1}<span class="pick"><Check size={12} /></span>{/if}
          {#if item.label}
            <span class="dot" style="background: {labelColors[item.label] ?? '#888'}"></span>
          {/if}
          <span class="name">{item.name}</span>
        </span>
      </button>
    {/each}
    </div>
  {/if}
</div>

{#if review}
  <ConfirmDialog
    title="Keep candidates and reject the rest"
    message={`Keep ${review.keepIds.length} ${review.keepIds.length === 1 ? "photo" : "photos"} (${review.keepFiles} ${review.keepFiles === 1 ? "file" : "files"}) and reject ${review.rejectIds.length} ${review.rejectIds.length === 1 ? "photo" : "photos"} (${review.rejectFiles} ${review.rejectFiles === 1 ? "file" : "files"}) in this batch. Rejects enter the delete queue. Files change only after you review and apply that queue.`}
    confirmLabel="Queue picks and rejects"
    onconfirm={() => session.applySurveyReview()}
    oncancel={() => { session.surveyReview = null; }}
  />
{/if}

<style>
  .survey {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 12px;
    color: #bbb;
  }

  .count {
    flex: none;
    font-variant-numeric: tabular-nums;
  }

  .capped {
    opacity: 0.55;
  }

  .hint {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.55;
  }

  header button {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-height: 28px;
    padding: 4px 7px;
    color: #ddd;
    background: var(--control);
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    font: inherit;
    cursor: pointer;
  }

  header button:disabled {
    opacity: 0.4;
    cursor: default;
  }

  header button:focus-visible,
  .tile:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }

  .close,
  .icon {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 24px;
    background: none;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    color: #bbb;
    cursor: pointer;
    padding: 0;
  }

  .close:hover {
    color: #fff;
  }

  .tiles {
    flex: 1;
    min-height: 0;
    display: grid;
    gap: 6px;
  }

  .detail {
    flex: 1;
    min-height: 0;
    position: relative;
    overflow: hidden;
  }

  .detail-name {
    position: absolute;
    bottom: 8px;
    left: 10px;
    color: #ddd;
    font-size: 12px;
    pointer-events: none;
  }

  .empty {
    flex: 1;
    display: grid;
    place-content: center;
    color: #bbb;
  }

  .tile {
    position: relative;
    min-width: 0;
    min-height: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: 2px solid transparent;
    border-radius: 6px;
    padding: 0;
    cursor: pointer;
    overflow: hidden;
  }

  .tile.focused {
    border-color: var(--accent);
  }

  .tile.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  img {
    max-width: 100%;
    max-height: 100%;
    object-fit: contain;
  }

  .meta {
    position: absolute;
    left: 4px;
    right: 4px;
    bottom: 4px;
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    color: #eee;
    text-shadow: 0 1px 3px rgba(0, 0, 0, 0.9);
    pointer-events: none;
  }

  .stars {
    color: #e0c34f;
    flex: none;
  }

  .pick {
    display: inline-flex;
    color: #59b85e;
    flex: none;
  }

  .dot {
    width: 9px;
    height: 9px;
    border-radius: 50%;
    flex: none;
  }

  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    opacity: 0.75;
  }

  @media (pointer: coarse) {
    header button {
      min-height: 44px;
      min-width: 44px;
    }

    .hint {
      display: none;
    }
  }
</style>
