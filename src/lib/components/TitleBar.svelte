<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { catalog } from "../stores/catalog.svelte";
  import { flushSessionSave } from "../stores/session.svelte";
  import { recent } from "../stores/recent.svelte";
  import Minus from "@lucide/svelte/icons/minus";
  import Square from "@lucide/svelte/icons/square";
  import Copy from "@lucide/svelte/icons/copy";
  import X from "@lucide/svelte/icons/x";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import LogOut from "@lucide/svelte/icons/log-out";
  import Album from "@lucide/svelte/icons/album";
  import RefreshCw from "@lucide/svelte/icons/refresh-cw";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Usb from "@lucide/svelte/icons/usb";
  import Network from "@lucide/svelte/icons/network";
  import Unplug from "@lucide/svelte/icons/unplug";
  import type { StorageKind } from "../api";

  // Storage-kind icon per classification (mirrors the recent-projects gallery).
  const kindIcon: Record<StorageKind, typeof HardDrive> = {
    internal: HardDrive,
    removable: Usb,
    network: Network,
    unknown: HardDrive,
  };

  // Project-management actions live in +page (which owns the open/close flow);
  // this bar just renders the menu and calls back.
  let {
    onOpenNew,
    onOpenRecent,
    onCloseProject,
    onRescan,
  }: {
    onOpenNew: () => void;
    onOpenRecent: (path: string) => void;
    onCloseProject: () => void;
    onRescan: () => void;
  } = $props();

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

  // Flush a still-debounced view/filter save before the window actually closes
  // (this button, Alt+F4, or the taskbar) — otherwise a change made in the last
  // 400ms before quitting never reaches the project's DB.
  $effect(() => {
    const unlisten = appWindow.onCloseRequested(async (event) => {
      event.preventDefault();
      await flushSessionSave();
      await appWindow.destroy();
    });
    return () => {
      void unlisten.then((f) => f());
    };
  });

  // Project switcher dropdown (JetBrains-style) — only shown while a project is
  // open. The name doubles as the menu trigger.
  let menuOpen = $state(false);

  // Up to five most-recently-opened projects other than the current one, for
  // quick switching. Refreshed each time the menu opens.
  const otherRecent = $derived(
    recent.list.filter((p) => p.path !== catalog.project?.rootPath).slice(0, 5),
  );

  let menuEl = $state<HTMLDivElement | null>(null);

  function toggleMenu() {
    menuOpen = !menuOpen;
    if (menuOpen) void recent.refresh();
  }

  // Focus the menu on open so Escape reaches onMenuKeydown; the trigger button
  // keeps focus after the click otherwise, and it is a sibling of .menu.
  $effect(() => {
    if (menuOpen) menuEl?.focus();
  });

  function onMenuKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      menuOpen = false;
      e.stopPropagation();
    }
  }

  function choose(fn: () => void) {
    menuOpen = false;
    fn();
  }
</script>

<div class="titlebar" data-tauri-drag-region>
  <div class="brand" data-tauri-drag-region>
    {#if catalog.project}
      <button
        class="project-btn"
        class:open={menuOpen}
        onclick={toggleMenu}
        title={catalog.project.rootPath}
      >
        <Album size={14} class="project-icon" />
        <span class="name">{catalog.project.displayName}</span>
        <ChevronDown size={13} />
      </button>
      {#if menuOpen}
        <!-- Full-window backdrop: an outside click closes the menu. -->
        <button class="menu-backdrop" aria-label="Close menu" onclick={() => (menuOpen = false)}></button>
        <div class="menu" role="menu" tabindex="-1" bind:this={menuEl} onkeydown={onMenuKeydown}>
          <button class="item" role="menuitem" onclick={() => choose(onOpenNew)}>
            <FolderOpen size={15} />
            <span>Open new project…</span>
          </button>
          {#if otherRecent.length > 0}
            <div class="sep"></div>
            <div class="menu-label">Recent</div>
            {#each otherRecent as p (p.path)}
              {@const RecentIcon =
                p.storage.state === "disconnected" ? Unplug : kindIcon[p.storage.kind]}
              <button
                class="item recent"
                role="menuitem"
                title={p.storage.state === "disconnected" && p.storage.volumeName
                  ? `${p.storage.volumeName} is not connected`
                  : p.path}
                disabled={p.storage.state !== "ok"}
                onclick={() => choose(() => onOpenRecent(p.path))}
              >
                <RecentIcon size={15} />
                <span class="recent-name">{p.displayName}</span>
              </button>
            {/each}
          {/if}
          <div class="sep"></div>
          <button
            class="item"
            role="menuitem"
            title="Rescan the project folder for added, removed or changed files"
            onclick={() => choose(onRescan)}
          >
            <RefreshCw size={15} />
            <span>Rescan project folder</span>
          </button>
          <button
            class="item danger"
            role="menuitem"
            title="Close the current project"
            onclick={() => choose(onCloseProject)}
          >
            <LogOut size={15} />
            <span>Close project</span>
          </button>
        </div>
      {/if}
    {:else}
      <span class="name idle" data-tauri-drag-region>Cullant</span>
    {/if}
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
    position: relative;
    display: flex;
    align-items: center;
    height: 100%;
    padding: 0 6px 0 8px;
    min-width: 0;
    flex: 1 1 auto;
  }

  .name {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: #e8e8e8;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .name.idle {
    padding: 0 4px;
  }

  /* Project name doubles as the menu trigger. */
  .project-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    max-width: 100%;
    padding: 0 7px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: #e8e8e8;
    font-family: inherit;
    cursor: pointer;
  }

  .project-btn > :global(svg) {
    flex: none;
    opacity: 0.6;
  }

  /* The leading project icon reads as an identity mark, so keep it clearer than
     the trailing dropdown chevron. */
  .project-btn > :global(.project-icon) {
    opacity: 0.85;
  }

  .project-btn:hover,
  .project-btn.open {
    background: var(--hover);
  }

  .menu-backdrop {
    position: fixed;
    inset: 0;
    z-index: 60;
    border: none;
    background: transparent;
    cursor: default;
  }

  .menu {
    /* The menu takes focus on open (so Escape works); suppress the focus ring. */
    outline: none;
    position: absolute;
    top: calc(100% + 2px);
    left: 6px;
    z-index: 61;
    min-width: 230px;
    max-width: 340px;
    display: flex;
    flex-direction: column;
    padding: 5px;
    background: #232329;
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
  }

  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    width: 100%;
    padding: 7px 9px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: #e8e8e8;
    font-family: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }

  .item > :global(svg) {
    flex: none;
    opacity: 0.7;
  }

  .item:hover:not(:disabled) {
    background: var(--hover);
  }

  /* Disabled rows (a recent project whose drive is disconnected or missing):
     an explicitly darker text/icon colour so it plainly reads as unavailable —
     dimming via opacity over the dark menu would make it fade lighter, not
     darker, which is the opposite of what's wanted. */
  .item:disabled {
    color: #5d5d64;
    cursor: default;
  }

  .item.danger:hover:not(:disabled) {
    background: rgba(224, 67, 67, 0.18);
    color: #ff9b9b;
  }

  .recent-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .menu-label {
    padding: 4px 9px 2px;
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    opacity: 0.45;
  }

  .sep {
    height: 1px;
    margin: 5px 4px;
    background: var(--border);
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
