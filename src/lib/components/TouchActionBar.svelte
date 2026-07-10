<script lang="ts">
  import { session } from "$lib/stores/session.svelte";
  import { runCommand } from "$lib/keyboard/dispatcher.svelte";
  import type { CommandId } from "$lib/keyboard/keymap";
  import ChevronLeft from "@lucide/svelte/icons/chevron-left";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import Flag from "@lucide/svelte/icons/flag";
  import Ban from "@lucide/svelte/icons/ban";
  import Star from "@lucide/svelte/icons/star";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import CheckCheck from "@lucide/svelte/icons/check-check";

  // The item the actions apply to (mirrors the keyboard path, which acts on the
  // focused item / current selection).
  const focused = $derived(session.focused);
  const rating = $derived(focused?.rating ?? 0);
  const flag = $derived(focused?.flag ?? 0);

  function rate(n: number) {
    // Tapping the current rating clears it, Lightroom-style.
    runCommand((rating === n ? "rate.0" : `rate.${n}`) as CommandId);
  }
</script>

<div class="touchbar" class:disabled={!focused}>
  <div class="group nav">
    <button class="btn" aria-label="Previous" onclick={() => runCommand("nav.prev")}>
      <ChevronLeft size={22} />
    </button>
    <button class="btn" aria-label="Next" onclick={() => runCommand("nav.next")}>
      <ChevronRight size={22} />
    </button>
  </div>

  <div class="group flags">
    <button
      class="btn"
      class:active-reject={flag === -1}
      aria-label="Reject"
      onclick={() => runCommand(flag === -1 ? "flag.unflag" : "flag.reject")}
    >
      <Ban size={20} />
    </button>
    <button
      class="btn"
      class:active-pick={flag === 1}
      aria-label="Pick"
      onclick={() => runCommand(flag === 1 ? "flag.unflag" : "flag.pick")}
    >
      <Flag size={20} />
    </button>
  </div>

  <div class="group stars">
    {#each [1, 2, 3, 4, 5] as n (n)}
      <button
        class="btn star"
        class:on={rating >= n}
        aria-label={`Rate ${n}`}
        onclick={() => rate(n)}
      >
        <Star size={18} fill={rating >= n ? "currentColor" : "none"} />
      </button>
    {/each}
  </div>

  <div class="group actions">
    <button class="btn danger" aria-label="Queue delete" onclick={() => runCommand("delete.pair")}>
      <Trash2 size={20} />
    </button>
    <button class="btn commit" aria-label="Review & commit" onclick={() => runCommand("commit.open")}>
      <CheckCheck size={20} />
    </button>
  </div>
</div>

<style>
  /* Touch-only: hidden where a precise pointer (mouse) is primary. */
  .touchbar {
    display: none;
  }

  @media (pointer: coarse) {
    .touchbar {
      display: flex;
      align-items: center;
      /* Center the button groups; "safe" falls back to start when the content
         overflows so the leading items stay reachable while scrolling. */
      justify-content: safe center;
      gap: 8px;
      overflow-x: auto;
      flex-wrap: nowrap;
      padding: 4px calc(10px + var(--safe-right)) calc(4px + var(--safe-bottom))
        calc(10px + var(--safe-left));
      background: var(--surface);
      border-top: 1px solid var(--border-strong);
      scrollbar-width: none;
    }
  }

  .touchbar::-webkit-scrollbar {
    display: none;
  }

  .touchbar.disabled {
    opacity: 0.45;
    pointer-events: none;
  }

  .group {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 0 0 auto;
  }

  .group + .group {
    padding-left: 10px;
    border-left: 1px solid var(--border);
  }

  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 44px;
    min-height: 42px;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    background: var(--control);
    color: #e8e8e8;
    padding: 0;
    -webkit-tap-highlight-color: transparent;
    touch-action: manipulation;
  }

  .btn:active {
    background: var(--hover);
  }

  .star {
    min-width: 40px;
    border: none;
    background: none;
    color: #5a5a63;
  }

  .star.on {
    color: #e0c34f;
  }

  .active-pick {
    color: #6be675;
    border-color: #6be675;
  }

  .active-reject {
    color: #ff6b6b;
    border-color: #ff6b6b;
  }

  .btn.danger:active {
    color: #ff6b6b;
  }

  .btn.commit {
    color: var(--accent);
    border-color: var(--border-strong);
  }
</style>
