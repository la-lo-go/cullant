<script lang="ts">
  import { api, previewUrl, thumbUrl, videoUrl, type ItemLite } from "../api";
  import { remuxForPlayback, type Remux } from "../video/remux";
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

  // Set once a clip has run out of ways to play: the WebView refused it AND the
  // in-app remux could not rescue it. Only then is the "open externally" escape
  // hatch the last word. Reset per clip in the item-change effect below.
  let failed = $state(false);

  // The source on the element: the file itself, unless the WebView turned that
  // down and the in-app remuxer produced a MediaSource it will accept.
  const nativeSrc = $derived(videoUrl(item));
  let remuxSrc = $state<string | null>(null);
  const src = $derived(remuxSrc ?? nativeSrc);
  // True while the remuxer is still feeding the MediaSource. Playback usually
  // starts long before this finishes — fragmented MP4 is playable as it arrives
  // — so this drives an unobtrusive indicator, not a blocking spinner.
  let remuxing = $state(false);
  let remuxProgress = $state(0);
  // The MediaError code the element reported, surfaced in the failure panel so
  // a field report names the problem instead of describing it.
  let mediaErrorCode = $state<number | null>(null);
  let remux: Remux | null = null;
  let triedRemux = false;

  // The element failed. Try to rewrite the clip into something it will accept
  // before giving up — see lib/video/remux.ts for why that so often works.
  async function onVideoError() {
    const failedItemId = item.id;
    // A WebView may emit `play` before the decoder rejects the first frame.
    // Restore the poster so neither that broken frame nor Chromium's broken-file
    // glyph becomes the backdrop for the remux/fallback UI.
    showPoster = true;
    mediaErrorCode = video?.error?.code ?? null;
    console.error("video playback failed", mediaErrorCode, video?.error?.message);
    // A second failure is the remuxed source failing too; nothing left to try.
    if (triedRemux) {
      failed = true;
      return;
    }
    triedRemux = true;
    remuxing = true;
    remuxProgress = 0;
    try {
      const handle = await remuxForPlayback(item, (f) => (remuxProgress = f));
      if (item.id !== failedItemId) {
        handle.cancel();
        return;
      }
      remux = handle;
      remuxSrc = handle.src;
      void handle.done.then(
        () => {
          if (item.id === failedItemId) remuxing = false;
        },
        (e) => {
          if (item.id !== failedItemId) return;
          console.error("in-app remux failed", e);
          remuxing = false;
          failed = true;
        },
      );
    } catch (e) {
      if (item.id !== failedItemId) return;
      console.error("in-app remux unavailable", e);
      remuxing = false;
      failed = true;
    }
  }

  $effect(() => () => remux?.cancel());

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
      sharp = new Image();
      sharp.onload = () => {
        if (cancelled || item.id !== itemId) return;
        sharpPoster = { itemId, src: sharpSrc };
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
      sharp.src = attempt === 0 ? sharpSrc : `${sharpSrc}&posterRetry=1`;
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
    sharpPosterFailed = false;
    volOpen = false;
    // Drop any remux belonging to the previous clip, along with its object URL.
    remux?.cancel();
    remux = null;
    remuxSrc = null;
    remuxing = false;
    mediaErrorCode = null;
    triedRemux = false;
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
    if (!video) return;
    if (video.paused) void video.play();
    else video.pause();
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
  {#key item.id}
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
      onclick={togglePlay}
      onplay={() => (showPoster = false)}
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
      <span>Decoding in app… {Math.round(remuxProgress * 100)}%</span>
    </div>
  {/if}

  {#if failed}
    <!-- Shown once the WebView refused the clip AND the in-app remux couldn't
         rescue it. Not part of the transport chrome, so it stays put while the
         (now useless) controls auto-hide. -->
    <div class="fallback">
      <TriangleAlert size={30} />
      <p>
        This video can't be played here.{#if mediaErrorCode}
          <br /><span class="code">Media error {mediaErrorCode}</span>
        {/if}
      </p>
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

  /* The raw MediaError code, kept quiet: useful in a bug report, noise to
     everyone else. */
  .fallback .code {
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.45);
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
