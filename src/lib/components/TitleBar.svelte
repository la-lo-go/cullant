<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import Copy from "@lucide/svelte/icons/copy";
  import X from "@lucide/svelte/icons/x";

  // Frameless custom chrome. Only ever mounted on desktop Windows (the parent
  // gates on platform), so `getCurrentWindow()` always has a real OS window
  // whose decorations were disabled in tauri.conf.json.
  const appWindow = getCurrentWindow();

  let maximized = $state(false);

  // Keep the maximize/restore icon in sync. A resize fires on every
  // maximize/unmaximize (and on snap/drag), so it's the reliable signal.
  $effect(() => {
    void appWindow.isMaximized().then((v) => (maximized = v));
    const unlisten = appWindow.onResized(() => {
      void appWindow.isMaximized().then((v) => (maximized = v));
    });
    return () => {
      void unlisten.then((f) => f());
    };
  });
</script>

<div class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    <span class="name" data-tauri-drag-region>Cullant</span>
  </div>
  <div class="controls">
    <button
      type="button"
      class="ctl"
      title="Minimize"
      aria-label="Minimize"
      onclick={() => void appWindow.minimize()}
    >
      <Minus size={16} />
    </button>
    <button
      type="button"
      class="ctl"
      title={maximized ? "Restore" : "Maximize"}
      aria-label={maximized ? "Restore" : "Maximize"}
      onclick={() => void appWindow.toggleMaximize()}
    >
      {#if maximized}
        <Copy size={14} />
      {:else}
        <Square size={14} />
      {/if}
    </button>
    <button
      type="button"
      class="ctl close"
      title="Close"
      aria-label="Close"
      onclick={() => void appWindow.close()}
    >
      <X size={16} />
    </button>
  </div>
</div>

<style>
  .titlebar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    flex: none;
    background: var(--surface-2);
    border-bottom: 1px solid var(--border);
    /* Keep OS text-selection off the drag surface. */
    user-select: none;
    -webkit-user-select: none;
  }

  .brand {
    display: flex;
    align-items: center;
    height: 100%;
    padding: 0 12px;
    min-width: 0;
    flex: 1 1 auto;
  }

  .name {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: #e8e8e8;
    white-space: nowrap;
  }

  .controls {
    display: flex;
    align-items: stretch;
    height: 100%;
    flex: none;
  }

  .ctl {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 46px;
    height: 100%;
    padding: 0;
    border: none;
    background: transparent;
    color: #e8e8e8;
    cursor: default;
    transition:
      background 0.12s ease,
      color 0.12s ease;
  }

  .ctl:hover {
    background: var(--hover);
  }

  .ctl.close:hover {
    background: #e04343;
    color: #ffffff;
  }
</style>
