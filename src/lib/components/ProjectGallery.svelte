<script lang="ts">
  import { recentThumbUrl, type RecentProject } from "../api";
  import { recent } from "../stores/recent.svelte";
  import ImageOff from "@lucide/svelte/icons/image-off";
  import X from "@lucide/svelte/icons/x";

  let { onopen }: { onopen: (path: string) => void } = $props();

  const PREVIEW_SLOTS = [0, 1, 2];

  $effect(() => {
    void recent.refresh();
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
          <div class="preview">
            {#if project.available}
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
              <ImageOff size={40} strokeWidth={1.25} />
            {/if}
          </div>
          <span class="name">{project.displayName}</span>
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

  .card:hover:not(:disabled) {
    background: var(--hover);
    border-color: var(--border-strong);
  }

  .card:disabled {
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
    background: var(--border-strong);
    color: #fff;
  }
</style>
