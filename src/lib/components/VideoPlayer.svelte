<script lang="ts">
  import { api, previewUrl, thumbUrl, videoUrl, type ItemLite } from "../api";
  import { untrack } from "svelte";
  import { needsPlaybackFallback, remuxForPlayback, UnplayableError, type Remux } from "../video/remux";
  import { unplayableVideos, playbackFailureKey } from "../video/failures";
  import Play from "@lucide/svelte/icons/play";
  import Pause from "@lucide/svelte/icons/pause";
  import Volume2 from "@lucide/svelte/icons/volume-2";
  import VolumeX from "@lucide/svelte/icons/volume-x";
  import Maximize from "@lucide/svelte/icons/maximize";
  import Minimize from "@lucide/svelte/icons/minimize";
  import ExternalLink from "@lucide/svelte/icons/external-link";
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

  // Set once a clip has run out of ways to play: the WebView refused it AND the
  // in-app remux could not rescue it. Only then is the "open externally" escape
  // hatch the last word. Reset per clip in the item-change effect below.
  const failureKey = $derived(playbackFailureKey(item));
  const rememberedFailure = $derived(unplayableVideos.has(failureKey));
  let currentFailure = $state(false);
  const failed = $derived(currentFailure || rememberedFailure);

  // The source on the element: the file itself, unless the WebView turned that
  // down and the in-app remuxer produced a MediaSource it will accept.
  const nativeSrc = $derived(videoUrl(item));
  let remuxSrc = $state<string | null>(null);
  const src = $derived(failed ? undefined : remuxSrc ?? nativeSrc);
  // True while the remuxer is still feeding the MediaSource. Playback usually
  // starts long before this finishes — fragmented MP4 is playable as it arrives
  // — so this drives an unobtrusive indicator, not a blocking spinner.
  let remuxing = $state(false);
  let remuxProgress = $state(0);
  let remux: Remux | null = null;
  let triedRemux = false;
  let remuxGeneration = 0;
  let playRequested = false;
  let recovering = false;
  let surfacePressed = false;
  let posterFrame: number | null = null;

  function cancelPosterFrame() {
    if (posterFrame === null) return;
    video?.cancelVideoFrameCallback(posterFrame);
    posterFrame = null;
  }

  function onPlaying() {
    // Android can emit `playing` without presenting a frame.
    const element = video;
    if (!element || failed || element.paused || element.ended) return;
    if (!element.requestVideoFrameCallback) {
      showPoster = false;
      return;
    }
    cancelPosterFrame();
    const generation = remuxGeneration;
    posterFrame = element.requestVideoFrameCallback(() => {
      posterFrame = null;
      if (element === video && generation === remuxGeneration && !failed && !element.paused) {
        showPoster = false;
      }
    });
  }

  function onSurfaceClick(e: MouseEvent) {
    // A grid pointerup mounts this video before Android sends its click.
    const pressed = surfacePressed;
    surfacePressed = false;
    if (pressed || e.detail === 0) togglePlay();
  }

  function isCurrent(generation: number, itemId: number) {
    return generation === remuxGeneration && item.id === itemId;
  }

  function failPlayback(error?: unknown) {
    cancelPosterFrame();
    currentFailure = true;
    if (!(error instanceof UnplayableError && error.retryable) && video?.error?.code !== 1) {
      unplayableVideos.add(failureKey);
    }
    showPoster = true;
    remuxing = false;
    playRequested = false;
    recovering = false;
    volOpen = false;
    video?.pause();
    remux?.cancel();
    remux = null;
  }

  // The element failed. Try to rewrite the clip into something it will accept
  // before giving up — see lib/video/remux.ts for why that so often works.
  async function onVideoError() {
    if (failed) return;
    cancelPosterFrame();
    const failedItemId = item.id;
    const generation = remuxGeneration;
    // A WebView may emit playback events before it rejects the first frame.
    // Restore the poster so neither that broken frame nor Chromium's broken-file
    // glyph becomes the backdrop for the remux/fallback UI.
    showPoster = true;
    const error = video?.error;
    console.error("video playback failed", error?.code, error?.message);
    // A second failure is the remuxed source failing too; nothing left to try.
    if (triedRemux) {
      // The native source can still fail while a proactive remux is pending.
      if (remuxSrc) failPlayback();
      return;
    }
    triedRemux = true;
    recovering = true;
    remuxProgress = 0;
    try {
      const handle = await remuxForPlayback(item, (f) => {
        if (generation === remuxGeneration && !failed) {
          remuxing = true;
          remuxProgress = f;
        }
      });
      if (failed || !isCurrent(generation, failedItemId)) {
        handle.cancel();
        return;
      }
      remux = handle;
      remuxSrc = handle.src;
      void handle.done.then(
        () => {
          if (generation === remuxGeneration) {
            remuxing = false;
            recovering = false;
          }
        },
        (e) => {
          if (!isCurrent(generation, failedItemId)) return;
          console.error("in-app remux failed", e);
          failPlayback(e);
        },
      );
    } catch (e) {
      if (!isCurrent(generation, failedItemId)) return;
      console.error("in-app remux unavailable", e);
      failPlayback(e);
    }
  }

  function checkCompatibility(element: HTMLVideoElement, currentItem: ItemLite, generation: number) {
    // Some Android decoders defer their error until Play. Inspect the actual
    // codec on entry so a known rejection does not require another tap.
    void needsPlaybackFallback(currentItem, (mime) => !!element.canPlayType(mime)).then((unsupported) => {
      if (unsupported && element === video && isCurrent(generation, currentItem.id) && !triedRemux && !failed) {
        void onVideoError();
      }
    }).catch((e) => console.error("video compatibility check unavailable", e));
  }

  $effect(() => () => {
    cancelPosterFrame();
    remuxGeneration += 1;
    remux?.cancel();
  });

  // Paint the cached grid thumbnail immediately, then replace it with the sharp
  // preview when its on-demand video-frame extraction finishes.
  let showPoster = $state(true);
  let posterFailed = $state(false);
  let sharpPosterFailed = $state(false);
  let sharpPoster = $state<{ itemId: number; src: string } | null>(null);
  const posterSrc = $derived(
    sharpPoster?.itemId === item.id ? sharpPoster.src : thumbUrl(item),
  );

  function onPosterError(itemId: number, failedSrc: string) {
    // Requests from the previous clip can finish after navigation. They must not
    // replace the new clip's pending sharp poster with the placeholder.
    if (item.id !== itemId || posterSrc !== failedSrc) return;
    if (sharpPoster?.itemId === itemId && sharpPoster.src === failedSrc) {
      sharpPosterFailed = true;
      sharpPoster = null;
      posterFailed = false;
      return;
    }
    posterFailed = true;
  }

  $effect(() => {
    const itemId = item.id;
    const sharpSrc = previewUrl(item);
    let sharp: HTMLImageElement | null = null;
    let retryTimer: ReturnType<typeof setTimeout> | null = null;
    let cancelled = false;
    sharpPosterFailed = false;

    const loadSharp = (attempt: number) => {
      const loadedSrc = attempt === 0 ? sharpSrc : `${sharpSrc}&posterRetry=1`;
      sharp = new Image();
      sharp.onload = () => {
        if (cancelled || item.id !== itemId) return;
        sharpPoster = { itemId, src: loadedSrc };
        sharpPosterFailed = false;
        posterFailed = false;
      };
      sharp.onerror = () => {
        if (cancelled || item.id !== itemId) return;
        // A first interactive poster request can be displaced by rapid grid
        // navigation. Retry once with a distinct URL before declaring the clip
        // genuinely undecodable.
        if (attempt === 0) {
          retryTimer = setTimeout(() => loadSharp(1), 250);
        } else {
          sharpPosterFailed = true;
        }
      };
      sharp.src = loadedSrc;
    };
    loadSharp(0);

    return () => {
      cancelled = true;
      if (retryTimer !== null) clearTimeout(retryTimer);
      if (sharp) {
        sharp.onload = null;
        sharp.onerror = null;
        sharp.src = "";
      }
    };
  });

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
  let controlsFocused = $state(false);
  let idleTimer: ReturnType<typeof setTimeout> | null = null;

  function clearIdle() {
    if (idleTimer !== null) {
      clearTimeout(idleTimer);
      idleTimer = null;
    }
  }

  // Focused controls must remain visible through playback and idle changes.
  function scheduleHide() {
    clearIdle();
    if (paused || hovering || controlsFocused) return;
    idleTimer = setTimeout(() => {
      idleTimer = null;
      if (!paused && !hovering && !controlsFocused) uiVisible = false;
    }, IDLE_MS);
  }

  // Any pointer activity (move or tap) reveals the UI and restarts the timer.
  function revealUi() {
    uiVisible = true;
    scheduleHide();
  }

  // Keep the UI pinned while paused; resume the idle countdown once playing.
  // Pointer hover and control focus cancel a pending hide. Leaving the controls
  // starts the timer again.
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
    void failureKey;
    cancelPosterFrame();
    remuxGeneration += 1;
    playRequested = false;
    surfacePressed = false;
    currentTime = 0;
    duration = 0;
    paused = true;
    uiVisible = true;
    hovering = false;
    currentFailure = false;
    showPoster = true;
    posterFailed = false;
    sharpPosterFailed = false;
    volOpen = false;
    // Drop any remux belonging to the previous clip, along with its object URL.
    remux?.cancel();
    remux = null;
    remuxSrc = null;
    remuxing = false;
    recovering = false;
    triedRemux = false;
    if (video && !untrack(() => rememberedFailure)) checkCompatibility(video, item, remuxGeneration);
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
    api.openExternal(item.id).catch((e) => console.error("open external failed", e));
  }

  function togglePlay() {
    if (!video || failed) return;
    playRequested = recovering ? !playRequested : video.paused;
    if (playRequested) void video.play().catch(() => {});
    else video.pause();
  }

  function resumeRequestedPlayback() {
    if (playRequested) void video?.play().catch(() => {});
  }

  async function toggleFullscreen() {
    if (!wrap) return;
    // requestFullscreen/exitFullscreen reject if the gesture is disallowed or the
    // element is gone; swallow it rather than leaving an unhandled rejection.
    try {
      if (document.fullscreenElement) await document.exitFullscreen();
      else await wrap.requestFullscreen();
    } catch (e) {
      console.error("fullscreen toggle failed", e);
    }
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

  // Keep native Space activation on controls. The player surface owns playback
  // Space, and neither route must reach the global zoom shortcut.
  function onKeydown(e: KeyboardEvent) {
    if (e.key === " " || e.code === "Space") {
      e.stopPropagation();
      if (e.target === e.currentTarget || e.target === video) {
        e.preventDefault();
        togglePlay();
      }
    }
  }

  // Pointer actions return to culling. Keyboard actions keep control focus.
  function refocus(e: MouseEvent | PointerEvent) {
    if (e.type === "pointerup" || e.detail > 0) wrap?.focus();
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
  {#key nativeSrc}
    <video
      class="player"
      bind:this={video}
      bind:paused
      bind:currentTime
      bind:duration
      bind:muted
      bind:volume
      {src}
      preload="metadata"
      playsinline
      onpointerdown={() => { surfacePressed = true; }}
      onpointercancel={() => { surfacePressed = false; }}
      onclick={onSurfaceClick}
      onplaying={onPlaying}
      onloadedmetadata={resumeRequestedPlayback}
      onended={() => { playRequested = false; }}
      onerror={onVideoError}
    ></video>
  {/key}

  {#if showPoster}
    <!-- Pre-playback poster so the clip doesn't open on a black frame. Passes
         pointer events through to the video below (a tap still plays). Kept on
         screen when playback fails, so the "can't be played" panel sits over the
         clip's own frame instead of over black — only playback actually starting
         clears it. -->
    <div class="video-poster">
      {#if posterFailed && sharpPosterFailed}
        <div class="no-poster">
          <Film size={48} />
          <span>{item.ext.toUpperCase()}</span>
        </div>
      {:else if posterFailed}
        <div class="poster-waiting" aria-hidden="true"></div>
      {:else}
        {#key `${item.id}:${posterSrc}`}
          <img src={posterSrc} alt="" onerror={() => onPosterError(item.id, posterSrc)} />
        {/key}
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

  {#if remuxing && !failed}
    <!-- The WebView refused the container and the in-app remuxer took over.
         Playback usually starts while this is still counting up, so it sits in
         a corner rather than over the frame. -->
    <div class="remuxing">
      <Film size={13} />
      <span>Preparing video… {Math.round(remuxProgress * 100)}%</span>
    </div>
  {/if}

  {#if failed}
    <div class="fallback" role="status">
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
    onfocusin={() => { controlsFocused = true; revealUi(); }}
    onfocusout={(e) => { controlsFocused = e.currentTarget.contains(e.relatedTarget as Node | null); }}
  >
    <button class="ctl" disabled={failed} title={failed ? "Playback unavailable" : paused ? "Play (Space)" : "Pause (Space)"} onclick={(e) => { togglePlay(); refocus(e); }}>
      {#if paused}<Play size={16} />{:else}<Pause size={16} />{/if}
    </button>

    <span class="time">{fmt(currentTime)}</span>

    <input
      class="seek"
      disabled={failed}
      type="range"
      min="0"
      max={duration || 0}
      step="0.01"
      value={currentTime}
      oninput={(e) => { if (video) video.currentTime = e.currentTarget.valueAsNumber; }}
      onpointerup={refocus}
      title="Seek"
    />

    <span class="time dur">{fmt(duration)}</span>

    <div class="vol-wrap">
      {#if volOpen}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div class="vol-popover" onpointerenter={() => { hovering = true; }} onpointerleave={() => { hovering = false; }}>
          <input
            class="vol"
            disabled={failed}
            type="range"
            min="0"
            max="1"
            step="0.01"
            value={muted ? 0 : volume}
            oninput={(e) => { volume = e.currentTarget.valueAsNumber; muted = volume === 0; }}
            onpointerup={refocus}
            title="Volume"
          />
        </div>
      {/if}
      <button class="ctl" disabled={failed} title="Volume" aria-label="Volume" onclick={(e) => { volOpen = !volOpen; refocus(e); }}>
        {#if muted || volume === 0}<VolumeX size={16} />{:else}<Volume2 size={16} />{/if}
      </button>
    </div>

    <button class="ctl" disabled={failed} title="Open in external player" onclick={(e) => { openExternally(); refocus(e); }}>
      <ExternalLink size={16} />
    </button>

    <button class="ctl" disabled={failed} title={fullscreen ? "Exit fullscreen" : "Fullscreen"} onclick={(e) => { void toggleFullscreen(); refocus(e); }}>
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

  /* Keep the playback warning below the poster's main subject. */
  .fallback {
    position: absolute;
    left: calc(10px + var(--safe-left));
    right: calc(10px + var(--safe-right));
    bottom: calc(82px + var(--safe-bottom));
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px;
    padding: 10px 12px;
    border-radius: 10px;
    color: rgba(255, 255, 255, 0.85);
    background: rgba(0, 0, 0, 0.8);
  }

  .fallback p {
    margin: 0;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.7);
  }

  /* Progress of the in-app remux. Tucked into the top corner, under the safe-area
     insets, so it never covers the frame that is already playing. */
  .remuxing {
    position: absolute;
    top: calc(10px + var(--safe-top));
    left: calc(10px + var(--safe-left));
    z-index: 4;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 5px 10px;
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(6px);
    color: rgba(255, 255, 255, 0.8);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    user-select: none;
    pointer-events: none;
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

  .controls:focus-within {
    opacity: 1;
    transform: none;
    pointer-events: auto;
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

  .ctl:enabled:hover {
    background: rgba(var(--accent-rgb), 0.5);
    color: #fff;
  }

  .ctl:disabled,
  .seek:disabled {
    opacity: 0.45;
    cursor: default;
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
