<script lang="ts">
  import { recentThumbUrl, type RecentProject, type StorageKind } from "../api";
  import { recent } from "../stores/recent.svelte";
  import ConfirmDialog from "./ConfirmDialog.svelte";
  import ImageOff from "@lucide/svelte/icons/image-off";
  import Trash2 from "@lucide/svelte/icons/trash-2";
  import Unplug from "@lucide/svelte/icons/unplug";
  import HardDrive from "@lucide/svelte/icons/hard-drive";
  import Usb from "@lucide/svelte/icons/usb";
  import Network from "@lucide/svelte/icons/network";

  let { onopen }: { onopen: (path: string) => void } = $props();

  const PREVIEW_SLOTS = [0, 1, 2];

  // Storage-kind badge icon per classification.
  const kindIcon: Record<StorageKind, typeof HardDrive> = {
    internal: HardDrive,
    removable: Usb,
    network: Network,
    unknown: HardDrive,
  };

  // Poll storage availability while the gallery is open. Desktop also reacts to
  // window focus / visibility changes (below), but Android gets no such event
  // when a volume is (un)plugged, so a short interval keeps the connected /
  // disconnected badges live without a manual rescan.
  $effect(() => {
    const id = setInterval(() => void recent.refresh(), 3000);
    return () => clearInterval(id);
  });

  // Project queued for full data deletion, awaiting confirmation (null = none).
  let deleteTarget = $state<RecentProject | null>(null);

  // Initial load when the gallery mounts (i.e. whenever we're on the homepage —
  // this component only renders while no project is open, so mounting already
  // covers "catalog.project became null").
  $effect(() => {
    void recent.refresh();
  });

  // Re-check folder/volume availability live: when the app window regains focus
  // or the tab becomes visible again (e.g. after unplugging/replugging a drive),
  // refresh the list so unavailable cards update without a manual reload.
  // Debounced so a burst of focus/visibility events triggers a single refresh.
  $effect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refreshSoon = () => {
      clearTimeout(timer);
      timer = setTimeout(() => void recent.refresh(), 150);
    };
    const onVisibility = () => {
      if (document.visibilityState === "visible") refreshSoon();
    };
    window.addEventListener("focus", refreshSoon);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      clearTimeout(timer);
      window.removeEventListener("focus", refreshSoon);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  });

  function relativeTime(unixSeconds: number): string {
    const diffMs = Date.now() - unixSeconds * 1000;
    const mins = Math.round(diffMs / 60000);
    if (mins < 1) return "just now";
    if (mins < 60) return `${mins}m ago`;
    const hours = Math.round(mins / 60);
    if (hours < 24) return `${hours}h ago`;
    const days = Math.round(hours / 24);
    if (days < 30) return `${days}d ago`;
    return new Date(unixSeconds * 1000).toLocaleDateString();
  }

  function openCard(project: RecentProject) {
    // Only a present project opens. Disconnected / not-found cards are inert
    // (disabled) — reconnect the volume and hit Rescan to re-check.
    if (project.storage.state === "ok") onopen(project.path);
  }

  function askDeleteCard(e: Event, project: RecentProject) {
    e.stopPropagation();
    deleteTarget = project;
  }

  function confirmDelete() {
    const path = deleteTarget?.path;
    deleteTarget = null;
    if (path) void recent.deleteProject(path);
  }
</script>

{#if recent.loaded && recent.list.length > 0}
  <div class="gallery">
    <div class="grid">
      {#each recent.list as project, index (project.path)}
        {@const st = project.storage}
        {@const KindIcon = kindIcon[st.kind]}
        <button
          class="card"
          class:unavailable={st.state !== "ok"}
          class:disconnected={st.state === "disconnected"}
          title={project.path}
          onclick={() => openCard(project)}
        >
          <span class="kind-badge" title={st.volumeName ?? st.kind}>
            <KindIcon size={13} />
          </span>
          <div class="preview">
            {#if st.state === "ok"}
              {#each PREVIEW_SLOTS as slot, i}
                <img
                  class="peek peek-{i}"
                  src={recentThumbUrl(index, slot, project.path)}
                  alt=""
                  loading="lazy"
                  onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = "none")}
                />
              {/each}
            {:else if st.state === "disconnected"}
              <Unplug size={38} strokeWidth={1.25} />
            {:else}
              <ImageOff size={40} strokeWidth={1.25} />
            {/if}
          </div>
          <span class="name">{project.displayName}</span>
          <span class="meta">
            {#if st.state === "ok"}
              Opened {relativeTime(project.lastOpened)}
            {:else if st.state === "disconnected"}
              Not connected{st.volumeName ? ` — ${st.volumeName}` : ""}
            {:else}
              Folder not found
            {/if}
          </span>
          <span
            class="delete"
            role="button"
            tabindex="-1"
            title="Delete Cullant data for this project (your photos are kept)"
            onclick={(e) => askDeleteCard(e, project)}
            onkeydown={(e) => {
              if (e.key !== "Enter" && e.key !== " ") return;
              e.preventDefault();
              askDeleteCard(e, project);
            }}
          >
            <Trash2 size={13} />
          </span>
        </button>
      {/each}
    </div>
  </div>
{/if}

{#if deleteTarget}
  <ConfirmDialog
    title="Delete Cullant data?"
    message={`This deletes Cullant's culling database and thumbnail cache for “${deleteTarget.displayName}” and removes it from recents. Your photos are NOT touched — only Cullant's own data is removed.`}
    confirmLabel="Delete Cullant data"
    onconfirm={confirmDelete}
    oncancel={() => (deleteTarget = null)}
  />
{/if}

<style>
  .gallery {
    flex: 1;
    overflow-y: auto;
    padding: 12px 24px 24px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(180px, 1fr));
    gap: 16px;
    max-width: 960px;
    margin: 0 auto;
  }


  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 14px 10px 10px;
    border-radius: 10px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: inherit;
    font-family: inherit;
    cursor: pointer;
  }

  .card:hover:not(.unavailable) {
    background: var(--hover);
    border-color: var(--border-strong);
  }

  /* Not a real `disabled` button: the card body no-ops on an unavailable
     project (openCard checks availability), but the delete action must stay
     clickable — a native `disabled` attribute would block all descendant
     click handlers too, not just the card's own. */
  .card.unavailable {
    cursor: default;
    opacity: 0.55;
  }

  /* A clean fanned stack of the project's photos (no folder, no glare). */
  .preview {
    position: relative;
    width: 132px;
    height: 96px;
    margin-bottom: 8px;
    display: flex;
    align-items: center;
    justify-content: center;
    color: #77777f;
  }

  .peek {
    position: absolute;
    width: 84px;
    height: 84px;
    object-fit: cover;
    border-radius: 5px;
    border: 2px solid var(--bg);
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.55);
    transition: transform 180ms ease-out;
  }

  .peek-0 {
    transform: translateX(-8px) rotate(-8deg);
    z-index: 1;
  }

  .peek-1 {
    transform: translateY(-2px) rotate(2deg);
    z-index: 3;
  }

  .peek-2 {
    transform: translateX(8px) rotate(9deg);
    z-index: 2;
  }

  .card:hover .peek-0 {
    transform: translateX(-32px) translateY(-4px) rotate(-14deg);
  }

  .card:hover .peek-1 {
    transform: translateY(-9px) rotate(2deg);
  }

  .card:hover .peek-2 {
    transform: translateX(32px) translateY(-4px) rotate(14deg);
  }

  .name {
    font-size: 13px;
    font-weight: 600;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    font-size: 11px;
    opacity: 0.55;
  }

  .card.unavailable .meta {
    color: #ff8f8f;
    opacity: 0.85;
  }

  /* Disconnected volume: disabled like a not-found one (via .unavailable), but
     amber rather than red to distinguish "not connected" from "folder gone". */
  .card.disconnected .meta {
    color: #ffcf8f;
    opacity: 0.9;
  }

  /* Always-on storage-kind badge (internal / removable / network), kept subtle
     and in the opposite corner from the delete action. */
  .kind-badge {
    position: absolute;
    top: 8px;
    left: 8px;
    display: inline-flex;
    color: #8a8a93;
    opacity: 0.5;
    pointer-events: none;
  }

  /* The only per-card action: deletes Cullant's own data (DB + thumb cache)
     for this project. Turns red on hover since it's destructive. */
  .delete {
    position: absolute;
    top: 4px;
    right: 4px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 5px;
    color: #999;
    opacity: 0;
  }

  .card:hover .delete {
    opacity: 1;
  }

  /* Touch has no hover: leave the button always visible there instead of
     requiring a tap-and-hold just to reveal it. */
  @media (pointer: coarse) {
    .delete {
      opacity: 1;
    }
  }

  .delete:hover {
    background: #a04040;
    color: #fff;
  }
</style>
