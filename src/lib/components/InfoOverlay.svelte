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

  // Wide screens only: park it under the icon and let the shared clamp pull it
  // back inside the window. The sheet spans the full width by construction.
  $effect(() => {
    const tip = view.infoTip;
    if (narrow || !tip || !popEl) return;
    popEl.style.left = `${tip.x}px`;
    popEl.style.top = `${tip.y}px`;
    return keepClamped(() => popEl);
  });
</script>

{#if view.infoTip}
  <button class="scrim" aria-label="Close explanation" onclick={() => (view.infoTip = null)}></button>
  <div class="pop" class:sheet={narrow} bind:this={popEl} role="tooltip">
    <span class="title">{view.infoTip.title}</span>
    <p>{view.infoTip.text}</p>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 90;
    border: 0;
    background: none;
    cursor: default;
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
