<script lang="ts">
  import { videoUrl, type ItemLite } from "../api";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Minimize from "@lucide/svelte/icons/minimize";

  let { item }: { item: ItemLite } = $props();

  let video = $state<HTMLVideoElement | null>(null);
  let wrap = $state<HTMLElement | null>(null);

  // Two-way bindings to the media element. Svelte keeps these live as the video
  // plays and seeks, so the transport UI is a pure reflection of element state.
  let paused = $state(true);
  let currentTime = $state(0);
  let duration = $state(0);
  let muted = $state(false);
  let volume = $state(1);
  let fullscreen = $state(false);

  // YouTube-style auto-hide of the transport chrome. `uiVisible` gates the bar's
  // opacity + the wrapper's cursor; `hovering` is true while the pointer sits on
  // the transport bar (so we never hide controls out from under the mouse). We
  // only ever auto-hide while playing — a paused clip keeps its UI on screen.
  const IDLE_MS = 2800;
  let uiVisible = $state(true);
  let hovering = $state(false);
  let idleTimer: ReturnType<typeof setTimeout> | null = null;

  function clearIdle() {
    if (idleTimer !== null) {
      clearTimeout(idleTimer);
      idleTimer = null;
    }
  }

  // Arm the hide timer. Hide only when playing && idle && not hovering a control.
  function scheduleHide() {
    clearIdle();
    if (paused || hovering) return;
    idleTimer = setTimeout(() => {
      idleTimer = null;
      if (!paused && !hovering) uiVisible = false;
    }, IDLE_MS);
  }

  // Any pointer activity (move or tap) reveals the UI and restarts the timer.
  function revealUi() {
    uiVisible = true;
    scheduleHide();
  }

  // Keep the UI pinned while paused; resume the idle countdown once playing.
  // Reading `paused` and `hovering` (via scheduleHide) makes this re-run when
  // either changes, so hovering a control cancels a pending hide and leaving it
  // re-arms the timer.
  $effect(() => {
    if (paused) {
      uiVisible = true;
      clearIdle();
    } else {
      scheduleHide();
    }
  });

  $effect(() => () => clearIdle());

  // Reset transport state whenever we navigate to a different clip. The element
  // is reused (same <video> node, new src), so nothing resets on its own.
  $effect(() => {
    void item.id; // re-run when the clip changes
    currentTime = 0;
    duration = 0;
    paused = true;
    uiVisible = true;
    hovering = false;
    // Grab focus so Space toggles playback immediately, without a prior click.
    wrap?.focus();
  });

  function togglePlay() {
    if (!video) return;
    if (video.paused) void video.play();
    else video.pause();
  }

  function toggleMute() {
    muted = !muted;
  }

  async function toggleFullscreen() {
    if (!wrap) return;
    if (document.fullscreenElement) await document.exitFullscreen();
    else await wrap.requestFullscreen();
  }

  function onFullscreenChange() {
    fullscreen = document.fullscreenElement === wrap;
  }

  function fmt(t: number): string {
    if (!Number.isFinite(t) || t < 0) t = 0;
    const s = Math.floor(t % 60);
    const m = Math.floor(t / 60) % 60;
    const h = Math.floor(t / 3600);
    const pad = (n: number) => n.toString().padStart(2, "0");
    return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
  }

  // Space toggles play/pause. We handle it on this focusable wrapper (bubble
  // phase, innermost element first) and stopPropagation so the global keyboard
  // dispatcher on window never also fires its Space binding. Every other key is
  // left untouched so grid navigation / rating shortcuts keep working.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === " " || e.code === "Space") {
      e.preventDefault();
      e.stopPropagation();
      togglePlay();
    }
  }

  // Toolbar controls steal DOM focus; releasing it back to the wrapper keeps
  // Space (and arrow navigation) working, mirroring +page.svelte's blurring().
  function refocus() {
    wrap?.focus();
  }
</script>

<svelte:window onfullscreenchange={onFullscreenChange} />

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="video-wrap"
  class:hide-cursor={!uiVisible}
  bind:this={wrap}
  tabindex="0"
  role="application"
  aria-label="Video player"
  onkeydown={onKeydown}
  onpointermove={revealUi}
  onpointerdown={revealUi}
>
  <!-- svelte-ignore a11y_media_has_caption -->
  <video
    class="player"
    bind:this={video}
    bind:paused
    bind:currentTime
    bind:duration
    bind:muted
    bind:volume
    src={videoUrl(item)}
    preload="metadata"
    onclick={togglePlay}
  ></video>

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="controls"
    class:hidden={!uiVisible}
    onpointerenter={() => { hovering = true; uiVisible = true; }}
    onpointerleave={() => { hovering = false; }}
  >
    <button class="ctl" title={paused ? "Play (Space)" : "Pause (Space)"} onclick={() => { togglePlay(); refocus(); }}>
      {#if paused}<Play size={16} />{:else}<Pause size={16} />{/if}
    </button>

    <span class="time">{fmt(currentTime)}</span>

    <input
      class="seek"
      type="range"
      min="0"
      max={duration || 0}
      step="0.01"
      value={currentTime}
      oninput={(e) => { if (video) video.currentTime = e.currentTarget.valueAsNumber; }}
      onchange={refocus}
      title="Seek"
    />

    <span class="time dur">{fmt(duration)}</span>

    <button class="ctl" title={muted ? "Unmute" : "Mute"} onclick={() => { toggleMute(); refocus(); }}>
      {#if muted || volume === 0}<VolumeX size={16} />{:else}<Volume2 size={16} />{/if}
    </button>

    <input
      class="vol"
      type="range"
      min="0"
      max="1"
      step="0.01"
      value={muted ? 0 : volume}
      oninput={(e) => { volume = e.currentTarget.valueAsNumber; muted = volume === 0; }}
      onchange={refocus}
      title="Volume"
    />

    <button class="ctl" title={fullscreen ? "Exit fullscreen" : "Fullscreen"} onclick={() => { void toggleFullscreen(); refocus(); }}>
      {#if fullscreen}<Minimize size={16} />{:else}<Maximize size={16} />{/if}
    </button>
  </div>
</div>

<style>
  .video-wrap {
    flex: 1;
    min-height: 0;
    position: relative;
    display: flex;
    outline: none;
  }

  .player {
    flex: 1;
    min-width: 0;
    min-height: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    background: var(--bg-stage);
    cursor: pointer;
  }

  /* Transport bar: same semi-transparent dark, rounded, pill language as the
     photo viewer's overlay buttons and zoom control. Lifted above the Viewer's
     bottom info strip and padded by the safe-area insets so it stays reachable
     under Android system bars / a display cutout. */
  .controls {
    position: absolute;
    left: calc(10px + var(--safe-left));
    right: calc(10px + var(--safe-right));
    bottom: calc(32px + var(--safe-bottom));
    z-index: 4;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 12px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(6px);
    transition:
      opacity 160ms ease,
      transform 160ms ease;
  }

  /* Auto-hidden while playing + idle. Fast fade/slide out; pointer-events off so
     an idle pointer resting over the (invisible) bar can't re-trigger hovering. */
  .controls.hidden {
    opacity: 0;
    transform: translateY(8px);
    pointer-events: none;
  }

  /* Hide the cursor along with the chrome whenever the UI is hidden. The video's
     own `cursor: pointer` must be overridden too. */
  .hide-cursor,
  .hide-cursor .player {
    cursor: none;
  }

  .ctl {
    flex: 0 0 auto;
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 6px;
    padding: 0;
    background: rgba(255, 255, 255, 0.08);
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
  }

  .ctl:hover {
    background: rgba(var(--accent-rgb), 0.5);
    color: #fff;
  }

  .time {
    flex: 0 0 auto;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.8);
    user-select: none;
  }

  .time.dur {
    opacity: 0.65;
  }

  .seek {
    flex: 1 1 auto;
    min-width: 60px;
    height: 12px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }

  .vol {
    flex: 0 0 auto;
    width: 72px;
    height: 12px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
</style>
