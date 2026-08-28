<script lang="ts">
  /**
   * The small ⓘ that carries a control's explanation.
   *
   * Labels stay short and the prose lives behind this, so a panel reads as a
   * list of settings rather than a wall of paragraphs. Pressing it hands the
   * text to `view.infoTip`, which InfoOverlay renders once for the whole app: one
   * explanation on screen at a time, and one thing for the back button to close.
   */
  import { view } from "../stores/view.svelte";
  import Info from "@lucide/svelte/icons/info";

  let {
    title,
    text,
    size = 13,
  }: {
    title: string;
    text: string;
    size?: number;
  } = $props();

  const open = $derived(view.infoTip?.title === title);

  // An explanation must not outlive the control it explains: a panel that closes
  // while its tip is up (Escape from inside it, a mode change, a project close)
  // takes the tip with it, wherever the panel lives.
  $effect(() => () => {
    if (view.infoTip?.title === title) view.infoTip = null;
  });

  function toggle(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    view.infoTip = open ? null : { title, text, x: r.left, y: r.bottom + 6 };
    // A pressed control must not keep the focus ring, and must not swallow the
    // next key press.
    (e.currentTarget as HTMLElement).blur();
  }
</script>

<!-- data-info-tip marks the button as owning its own click, so the overlay's
     dismiss-on-press leaves toggling (and swapping) explanations alone. -->
<button class="tip" class:open data-info-tip aria-label="What does {title} do?" onclick={toggle}>
  <Info {size} />
</button>

<style>
  .tip {
    display: inline-flex;
    flex: none;
    padding: 3px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: inherit;
    opacity: 0.35;
    cursor: pointer;
  }

  .tip:hover,
  .tip.open {
    opacity: 1;
    color: var(--accent);
  }
</style>
