<script lang="ts">
  /**
   * Renders whatever explanation an InfoTip has opened. Mounted once, at the top
   * of the page, so it floats above every dialog and panel that can host a tip.
   *
   * Anchored under its icon on a wide screen. On a narrow one it becomes a
   * full-width bottom sheet: a popover pinned near the right edge of a phone
   * leaves the text a few characters wide.
   */
  import { view } from "../stores/view.svelte";
  import { keepClamped } from "../popover";

  const NARROW_QUERY = "(max-width: 600px)";
  let narrow = $state(false);
  $effect(() => {
    const mq = window.matchMedia(NARROW_QUERY);
    narrow = mq.matches;
    const onChange = () => (narrow = mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  });

  let popEl = $state<HTMLDivElement | null>(null);

  /**
   * Any click outside the explanation dismisses it, and still reaches whatever
   * it landed on: closing the host panel from its backdrop or its X is one tap,
   * not one to shed the explanation and another to close the panel. The scrim
   * below stops the clicks that have nothing under them.
   *
   * Capture phase, because a dialog that stops clicks from bubbling (the
   * settings dialog does) would otherwise hide every press inside it.
   */
  $effect(() => {
    if (!view.infoTip) return;
    const onClick = (e: MouseEvent) => {
      const target = e.target as Element | null;
      if (!target || target.closest("[data-info-tip]") || popEl?.contains(target)) return;
      view.infoTip = null;
    };
    window.addEventListener("click", onClick, true);
    return () => window.removeEventListener("click", onClick, true);
  });

  // Wide screens only: park it under the icon and let the shared clamp pull it
  // back inside the window. The sheet spans the full width by construction.
  $effect(() => {
    const tip = view.infoTip;
    const element = popEl;
    if (narrow || !tip || !element) return;
    element.style.left = `${tip.x}px`;
    element.style.top = `${tip.y}px`;
    const stop = keepClamped(() => element);
    return () => {
      stop();
      element.style.left = "";
      element.style.top = "";
      element.style.transform = "";
      element.style.maxHeight = "";
    };
  });
</script>

{#if view.infoTip}
  <button class="scrim" aria-label="Close explanation" onclick={() => (view.infoTip = null)}></button>
  <!-- The sheet covers the bottom of a phone, so the app behind it is dimmed to
       say which layer is live. A popover parked under its own icon is already
       tied to what it explains: dimming the window around it, and repeating the
       label the icon sits next to, only make a one-line aside feel like a
       modal. -->
  {#if narrow}
    <div class="dim" aria-hidden="true"></div>
  {/if}
  <div class="pop" class:sheet={narrow} bind:this={popEl} role="tooltip">
    {#if narrow}
      <span class="title">{view.infoTip.title}</span>
    {/if}
    <p>{view.infoTip.text}</p>
  </div>
{/if}

<style>
  /* Swallows the clicks that would otherwise act on the app underneath (a
     thumbnail, the touch bar). Deliberately BELOW the panel layer: a press on a
     panel's backdrop or on its X has to reach the panel, so the same tap that
     sheds the explanation closes the panel too. */
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 35;
    border: 0;
    background: none;
    cursor: default;
  }

  /* The dimming is its own layer, above the panels the scrim sits under, and it
     never takes a press. Same value the dialog backdrops use. */
  .dim {
    position: fixed;
    inset: 0;
    z-index: 90;
    pointer-events: none;
    background: rgba(0, 0, 0, 0.55);
  }

  .pop {
    position: fixed;
    z-index: 91;
    width: 300px;
    max-width: calc(100vw - var(--dialog-edge-margin) * 2);
    overflow-y: auto;
    padding: 10px 12px;
    background: #2c2c33;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
  }

  .pop.sheet {
    left: 0;
    right: 0;
    bottom: 0;
    top: auto;
    width: auto;
    max-width: none;
    max-height: calc(100dvh - var(--dialog-edge-margin) * 2);
    padding: 16px 18px calc(20px + var(--inset-bottom));
    border-width: 1px 0 0;
    border-radius: 14px 14px 0 0;
  }

  .title {
    display: block;
    margin-bottom: 5px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
  }

  p {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    opacity: 0.82;
  }
</style>
