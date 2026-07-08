<script lang="ts">
  import { recentThumbUrl, type RecentProject } from "../api";
  import { recent } from "../stores/recent.svelte";
  import Folder from "@lucide/svelte/icons/folder";
  import FolderX from "@lucide/svelte/icons/folder-x";
  import X from "@lucide/svelte/icons/x";

  let { onopen }: { onopen: (path: string) => void } = $props();

  const PREVIEW_SLOTS = [0, 1, 2];

  $effect(() => {
    void recent.refresh();
  });

  function folderName(path: string): string {
    const segments = path.split(/[\\/]+/).filter(Boolean);
    return segments[segments.length - 1] ?? path;
  }

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
    if (project.available) onopen(project.path);
  }

  function removeCard(e: Event, path: string) {
    e.stopPropagation();
    void recent.remove(path);
  }
</script>

{#if recent.loaded && recent.list.length > 0}
  <div class="gallery">
    <div class="grid">
      {#each recent.list as project, index (project.path)}
        <button
          class="card"
          class:unavailable={!project.available}
          title={project.path}
          onclick={() => openCard(project)}
          disabled={!project.available}
        >
          <div class="folder-icon">
            {#if project.available}
              <Folder size={64} strokeWidth={1} />
              {#each PREVIEW_SLOTS as slot, i}
                <img
                  class="peek peek-{i}"
                  src={recentThumbUrl(index, slot)}
                  alt=""
                  loading="lazy"
                  onerror={(e) => ((e.currentTarget as HTMLImageElement).style.display = "none")}
                />
              {/each}
            {:else}
              <FolderX size={64} strokeWidth={1} />
            {/if}
          </div>
          <span class="name">{folderName(project.path)}</span>
          <span class="meta">
            {#if project.available}
              Opened {relativeTime(project.lastOpened)}
            {:else}
              Folder not found
            {/if}
          </span>
          <span
            class="remove"
            role="button"
            tabindex="-1"
            title="Remove from recent projects"
            onclick={(e) => removeCard(e, project.path)}
            onkeydown={(e) => {
              if (e.key !== "Enter" && e.key !== " ") return;
              e.preventDefault();
              removeCard(e, project.path);
            }}
          >
            <X size={13} />
          </span>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .gallery {
    flex: 1;
    overflow-y: auto;
    padding: 12px 24px 24px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 14px;
    max-width: 900px;
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
    border: 1px solid transparent;
    background: transparent;
    color: inherit;
    font-family: inherit;
    cursor: pointer;
  }

  .card:hover:not(:disabled) {
    background: #232329;
    border-color: #3a3a42;
  }

  .card:disabled {
    cursor: default;
    opacity: 0.55;
  }

  .folder-icon {
    position: relative;
    width: 64px;
    height: 64px;
    color: #6b8bff;
    margin-bottom: 6px;
  }

  .card.unavailable .folder-icon {
    color: #77777f;
  }

  .peek {
    position: absolute;
    width: 28px;
    height: 28px;
    object-fit: cover;
    border-radius: 3px;
    border: 2px solid #1b1b1f;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.5);
    top: 6px;
  }

  .peek-0 {
    left: 6px;
    transform: rotate(-8deg);
    z-index: 3;
  }

  .peek-1 {
    left: 22px;
    transform: rotate(3deg);
    z-index: 2;
  }

  .peek-2 {
    left: 36px;
    transform: rotate(11deg);
    z-index: 1;
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

  .remove {
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

  .card:hover .remove {
    opacity: 1;
  }

  .remove:hover {
    background: #3a3a42;
    color: #fff;
  }
</style>
