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

  /** Placeholder colours for a card with no cached thumbnails. Pastels, so they
   *  read as "photo not here yet" rather than as an error state. */
  const PLACEHOLDER_COLORS = [
    "#8ab6d6",
    "#a3c9a8",
    "#e3b7a0",
    "#c9a7d4",
    "#e6c98f",
    "#92c7c0",
    "#d99a9a",
    "#b0b8e0",
    "#c7cf9c",
    "#e0a8c4",
    "#9fc6e0",
    "#d4b48c",
    "#a9d1b8",
    "#cbb2e8",
    "#e8c4a0",
  ];
  /** Strides coprime with the palette length, so the three slots of one card
   *  can never land on the same colour. */
  const COLOR_STRIDES = [1, 2, 4, 7];

  function hashPath(path: string): number {
    let h = 0;
    for (let i = 0; i < path.length; i++) h = (h * 31 + path.charCodeAt(i)) | 0;
    return Math.abs(h);
  }

  /** Pseudo-random, but derived from the path so a card keeps its colours
   *  across re-renders instead of reshuffling under the pointer. */
  function placeholderColor(path: string, slot: number): string {
    const h = hashPath(path);
    const stride = COLOR_STRIDES[h % COLOR_STRIDES.length];
    return PLACEHOLDER_COLORS[(h + slot * stride) % PLACEHOLDER_COLORS.length];
  }

  // Storage-kind badge icon per classification.
  const kindIcon: Record<StorageKind, typeof HardDrive> = {
    internal: HardDrive,
    removable: Usb,
    network: Network,
    unknown: HardDrive,
  };

  // Poll storage availability while the gallery is open. Desktop already reacts
  // to window focus / visibility changes (below), so the poll is only for touch
  // devices (Android), which get no such event when a volume is (un)plugged.
  $effect(() => {
    if (!window.matchMedia("(pointer: coarse)").matches) return;
    const id = setInterval(
      () => recent.refresh().catch((e) => console.error("recent refresh failed", e)),
      3000,
    );
    return () => clearInterval(id);
  });

  // Project queued for full data deletion, awaiting confirmation (null = none).
  let deleteTarget = $state<RecentProject | null>(null);

  // Initial load when the gallery mounts (i.e. whenever we're on the homepage —
  // this component only renders while no project is open, so mounting already
  // covers "catalog.project became null").
  $effect(() => {
    recent.refresh().catch((e) => console.error("recent refresh failed", e));
  });

  // Re-check folder/volume availability live: when the app window regains focus
  // or the tab becomes visible again (e.g. after unplugging/replugging a drive),
  // refresh the list so unavailable cards update without a manual reload.
  // Debounced so a burst of focus/visibility events triggers a single refresh.
  $effect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refreshSoon = () => {
      clearTimeout(timer);
      timer = setTimeout(
        () => recent.refresh().catch((e) => console.error("recent refresh failed", e)),
        150,
      );
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
    if (path) recent.deleteProject(path).catch((e) => console.error("delete project failed", e));
  }
</script>

{#if recent.loaded && recent.list.length > 0}
  <div class="gallery">
    <div class="grid">
      {#each recent.list as project, index (project.path)}
        {@const st = project.storage}
        {@const KindIcon = kindIcon[st.kind]}
        <!-- The delete control is a sibling of the card button, not a child:
             nesting an interactive element inside a <button> is invalid HTML and
             leaves it keyboard-unreachable. -->
        <div class="card-wrap">
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
                  <!-- The placeholder sits under its thumbnail and the opaque
                       thumbnail covers it. Hiding a thumbnail that never arrives
                       therefore reveals a coloured card instead of a hole — for a
                       project whose import has not reached the previews yet, and
                       for one that simply holds fewer photos than there are slots. -->
                  <span
                    class="peek peek-{i} placeholder"
                    style="background: {placeholderColor(project.path, slot)}"
                  >
                    <!-- The app mark, watermarked into the card so an empty slot
                         still looks like something Cullant drew on purpose. -->
                    <svg class="mark" viewBox="0 0 1369 1218" aria-hidden="true">
                      <g transform="matrix(1,0,0,1,-1069.617379,-135.415161)">
                        <g transform="matrix(3.103723,0,0,3.103723,154.590885,-721.515314)">
                          <path
                            d="M735.586,519.5C735.586,519.5 739.473,568.534 712.678,582.843C680.798,599.868 663.519,553.11 634.642,577.247C611.326,596.735 629.032,625.109 626.151,643.124C619.985,681.67 555.152,677.363 569.967,621.314C571.184,616.713 584.535,562.904 535.024,552.746C518.381,549.332 479.567,550.468 480.73,596.91C481.01,608.097 487.504,638.968 460.464,643.631C448.06,645.77 432.423,635.11 434.118,613.897C435.531,596.208 442.333,576.751 407.462,568.666C377.23,561.657 353.197,588.486 328.527,589.882C325.237,590.069 298.431,591.586 295.307,565.522C292.837,544.908 300.402,535.202 302.029,533.113C306.097,527.894 391.343,432.74 394.697,430.789C409.535,422.16 419.042,435.709 427.892,444.083C432.306,448.259 432.329,448.152 436.673,452.323C472.319,486.545 472.587,486.553 475.559,487.197C484.075,489.039 482.846,484.581 509.213,452.274C528.631,428.482 527.26,427.49 546.592,403.57C563.401,382.77 562.188,381.933 579.285,361.329C587.143,351.86 592.61,338.702 606.33,342.985C611.957,344.741 611.874,347.567 646.265,390.682C660.202,408.155 678.899,432.673 681.703,436.351C708.771,471.847 731.085,495.961 734.185,506.594C736.027,512.914 735.586,519.5 735.586,519.5Z"
                          />
                          <g transform="matrix(0.813344,0,0,0.813344,-674.196775,-1509.93443)">
                            <circle cx="1324.072" cy="2261.054" r="65.142" />
                          </g>
                        </g>
                      </g>
                    </svg>
                  </span>
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
                {st.volumeName ? `${st.volumeName} is not connected` : "Not connected"}
              {:else}
                Folder not found
              {/if}
            </span>
          </button>
          <button
            class="delete"
            title="Delete Cullant data for this project (your photos are kept)"
            aria-label="Delete Cullant data for this project"
            onclick={(e) => askDeleteCard(e, project)}
          >
            <Trash2 size={14} />
          </button>
        </div>
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


  .card-wrap {
    position: relative;
  }

  .card {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    width: 100%;
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

  .placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    box-sizing: border-box;
  }

  /* White on a light pastel reads as a watermark rather than as a label, which
     is what keeps the empty slot quiet next to cards showing real photos. */
  .mark {
    width: 46%;
    height: auto;
    fill: #fff;
    opacity: 0.55;
    fill-rule: evenodd;
    clip-rule: evenodd;
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
     for this project. Turns red on hover since it's destructive. Sized like the
     app's other icon buttons (36px, 44px on touch) so it is easy to hit, with a
     chip background because it sits over the photo stack once the card fans out. */
  .delete {
    position: absolute;
    top: 4px;
    right: 4px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: none;
    background: none;
    border-radius: 7px;
    color: #d8d8de;
    opacity: 0;
    /* Hidden means inert. Without this the invisible button still takes the
       clicks aimed at the card corner, and at this size it would eat a real
       slice of the area that opens the project. */
    pointer-events: none;
    cursor: pointer;
  }

  .card-wrap:hover .delete,
  .delete:focus-visible {
    opacity: 1;
    pointer-events: auto;
  }

  /* Touch has no hover: leave the button always visible there instead of
     requiring a tap-and-hold just to reveal it, and give it the full 44px
     target the rest of the app uses on coarse pointers. */
  @media (pointer: coarse) {
    .delete {
      width: 44px;
      height: 44px;
      opacity: 1;
      pointer-events: auto;
    }
    /* The desktop icon is sized for a 26px button; keep it legible in the 44px
       touch target rather than leaving it adrift in the middle. */
    .delete :global(svg) {
      width: 18px;
      height: 18px;
    }
  }

  .delete:hover {
    background: #a04040;
    color: #fff;
  }
</style>
