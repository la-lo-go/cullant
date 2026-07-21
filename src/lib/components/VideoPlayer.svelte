<script lang="ts">
  import { api, previewUrl, videoUrl, type ItemLite } from "../api";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Minimize from "@lucide/svelte/icons/minimize";
  import ExternalLink from "@lucide/svelte/icons/external-link";
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import Film from "@lucide/svelte/icons/film";

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

  // Set when the WebView reports it can't decode this clip (a MediaError on the
  // element). Android's built-in media stack supports far fewer codecs than a
  // desktop browser, so HEVC/other clips can fail here even though they play
  // fine in a native app — hence the "open externally" escape hatch. Reset per
  // clip in the item-change effect below.
  let failed = $state(false);

  // Poster shown over the (paused, pre-playback) video so the clip never opens on
  // a black frame. We paint the generated thumbnail; if it 404s — no poster was
  // ever generated (ffmpeg absent / undecodable) — we fall back to the same
  // film-glyph placeholder the grid uses. Cleared once playback starts, and reset
  // per clip in the item-change effect below.
  let showPoster = $state(true);
  let posterFailed = $state(false);

  // The volume slider is hidden until the speaker button is pressed, then pops up
  // in a small popover above the button (touch-friendly: no permanent slider
  // eating bar width). Closed on outside click and when the chrome auto-hides.
  let volOpen = $state(false);

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
    failed = false;
    showPoster = true;
    posterFailed = false;
    volOpen = false;
    // Grab focus so Space toggles playback immediately, without a prior click.
    wrap?.focus();
  });

  // The volume popover has no business staying open once the chrome hides.
  $effect(() => {
    if (!uiVisible) volOpen = false;
  });

  // Hand the clip to an external app (system default player). The reliable
  // escape hatch when in-app decode fails; also offered pre-emptively so the
  // user is never stuck on a silent black frame the WebView never errors on.
  function openExternally() {
    void api.openExternal(item.id);
  }

  function togglePlay() {
    if (!video) return;
    if (video.paused) void video.play();
    else video.pause();
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
    playsinline
    onclick={togglePlay}
    onplay={() => (showPoster = false)}
    onerror={() => (failed = true)}
  ></video>

  {#if showPoster && !failed}
    <!-- Pre-playback poster so the clip doesn't open on a black frame. Passes
         pointer events through to the video below (a tap still plays). -->
    <div class="video-poster">
      {#if posterFailed}
        <div class="no-poster">
          <Film size={48} />
          <span>{item.ext.toUpperCase()}</span>
        </div>
      {:else}
        <img src={previewUrl(item)} alt="" onerror={() => (posterFailed = true)} />
      {/if}
    </div>
  {/if}

  {#if volOpen}
    <!-- Outside-click catcher for the volume popover (same trick as the title
         bar's menu backdrop). Sits below the transport bar so its controls stay
         live; a click anywhere else closes the popover. -->
    <button class="vol-backdrop" aria-label="Close volume" onclick={() => (volOpen = false)}
    ></button>
  {/if}

  {#if failed}
    <!-- Shown when the WebView can't decode the clip. Not part of the transport
         chrome, so it stays put while the (now useless) controls auto-hide. -->
    <div class="fallback">
      <TriangleAlert size={30} />
      <p>This video can't be played here.</p>
      <button class="open-ext" onclick={openExternally}>
        <ExternalLink size={16} />
        <span>Open in external player</span>
      </button>
    </div>
  {/if}

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

    <div class="vol-wrap">
      {#if volOpen}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="vol-popover" onpointerenter={() => { hovering = true; }} onpointerleave={() => { hovering = false; }}>
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
        </div>
      {/if}
      <button class="ctl" title="Volume" aria-label="Volume" onclick={() => { volOpen = !volOpen; refocus(); }}>
        {#if muted || volume === 0}<VolumeX size={16} />{:else}<Volume2 size={16} />{/if}
      </button>
    </div>

    <button class="ctl" title="Open in external player" onclick={() => { openExternally(); refocus(); }}>
      <ExternalLink size={16} />
    </button>

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

  /* Centered "can't decode — open externally" panel. Sits above the video
     (which is showing nothing useful) but below the transport bar's z-index so
     the controls stay reachable. Same pill/backdrop language as the chrome. */
  .fallback {
    position: absolute;
    inset: 0;
    z-index: 3;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    padding: 24px;
    text-align: center;
    color: rgba(255, 255, 255, 0.85);
    background: rgba(0, 0, 0, 0.4);
    backdrop-filter: blur(2px);
  }

  .fallback p {
    margin: 0;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.7);
  }

  .open-ext {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 8px 16px;
    border: none;
    border-radius: 8px;
    background: rgba(var(--accent-rgb), 0.85);
    color: #fff;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .open-ext:hover {
    background: var(--accent);
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

  /* Pre-playback poster over the video. Mirrors the video's own contain-fit on
     the same dark stage, so swapping poster→frame is seamless. Click-through so a
     tap on the frame still starts playback. */
  .video-poster {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-stage);
    pointer-events: none;
  }

  .video-poster img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  /* No poster was ever generated — same film-glyph placeholder as the grid. */
  .no-poster {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    color: #8a8a93;
    user-select: none;
  }

  .no-poster span {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.03em;
  }

  /* Anchors the volume popover above the speaker button. */
  .vol-wrap {
    position: relative;
    flex: 0 0 auto;
    display: inline-flex;
  }

  .vol-backdrop {
    position: fixed;
    inset: 0;
    z-index: 3;
    border: none;
    background: transparent;
    cursor: default;
  }

  /* Floating volume panel: pops up centered above the speaker button, above the
     transport bar's own z-index so it's never clipped by the bar. */
  .vol-popover {
    position: absolute;
    bottom: calc(100% + 8px);
    left: 50%;
    transform: translateX(-50%);
    z-index: 5;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 10px 8px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.85);
    backdrop-filter: blur(6px);
  }

  /* Vertical slider (modern writing-mode approach; Android WebView is Chromium). */
  .vol {
    writing-mode: vertical-lr;
    direction: rtl;
    width: 12px;
    height: 90px;
    margin: 0;
    accent-color: var(--accent);
    cursor: pointer;
  }
</style>
