<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { api, type FileMetadata, type ItemLite } from "../api";
  import { view } from "../stores/view.svelte";
  import X from "@lucide/svelte/icons/x";
  import MapPin from "@lucide/svelte/icons/map-pin";

  let { item }: { item: ItemLite } = $props();

  let meta = $state<FileMetadata | null>(null);
  let loading = $state(false);

  $effect(() => {
    const id = item.id;
    loading = true;
    meta = null;
    api
      .getFileMetadata(id)
      .then((m) => {
        // Guard against a late response for a photo we already stepped past.
        if (id === item.id) meta = m;
      })
      .catch((e) => {
        // A failed read must not leave the panel stuck on "Reading…"; fall back
        // to the empty state for the current photo.
        if (id === item.id) {
          console.error("metadata read failed", e);
          meta = null;
        }
      })
      .finally(() => {
        // Stepping fast can resolve an earlier request while a newer one is in
        // flight; only the current item's request may clear the loading flag.
        if (id === item.id) loading = false;
      });
  });

  function fmtSize(bytes: number): string {
    if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(0)} KB`;
    return `${bytes} B`;
  }

  function fmtDate(ts: number | null): string {
    if (!ts) return "—";
    return new Date(ts * 1000).toLocaleString();
  }

  const gpsUrl = $derived(
    meta && meta.gpsLat != null && meta.gpsLon != null
      ? `https://www.google.com/maps/search/?api=1&query=${meta.gpsLat},${meta.gpsLon}`
      : null
  );

  const rows = $derived.by(() => {
    if (!meta) return [];
    const dims =
      meta.width && meta.height ? `${meta.width} × ${meta.height}` : "—";
    const all: [string, string][] = [
      ["Camera", meta.camera ?? "—"],
      ["Lens", meta.lens ?? "—"],
      ["Focal", meta.focalLength ?? "—"],
      ["Aperture", meta.fNumber ?? "—"],
      ["Shutter", meta.exposureTime ?? "—"],
      ["ISO", meta.iso != null ? String(meta.iso) : "—"],
      ["Exp. comp.", meta.exposureBias ?? "—"],
      ["Flash", meta.flash ?? "—"],
      ["Dimensions", dims],
      ["Size", fmtSize(meta.size)],
      ["Captured", fmtDate(meta.captureTime)],
    ];
    return all.filter(([, value]) => value !== "—");
  });

  let panelEl = $state<HTMLElement | null>(null);

  function handleOutsideClick(e: PointerEvent) {
    const target = e.target as HTMLElement | null;
    if (!panelEl || !target) return;
    if (panelEl.contains(target) || target.closest("[data-metadata-toggle]")) return;
    view.infoOpen = false;
  }
</script>

<svelte:window onpointerdown={handleOutsideClick} />

<aside class="panel" bind:this={panelEl}>
  <header>
    <span>Metadata</span>
    <button class="close" title="Close (I)" onclick={() => (view.infoOpen = false)}>
      <X size={14} />
    </button>
  </header>

  {#if loading && !meta}
    <p class="muted">Reading…</p>
  {:else if meta}
    <dl>
      {#each rows as [label, value]}
        <dt>{label}</dt>
        <dd>{value}</dd>
      {/each}
    </dl>
    {#if gpsUrl}
      <button class="gps" onclick={() => openUrl(gpsUrl)}>
        <MapPin size={13} />
        {meta.gpsLat!.toFixed(5)}, {meta.gpsLon!.toFixed(5)}
      </button>
    {/if}
    <p class="path" title={meta.relPath}>{meta.relPath}</p>
  {/if}
</aside>

<style>
  .panel {
    position: absolute;
    top: 10px;
    right: calc(48px + var(--safe-right));
    width: 260px;
    /* Leave at least the shared dialog edge margin on the left, and subtract
       the safe-area insets that 100vw includes but the (already inset) viewer
       does not, so the panel can't slide under a cutout in landscape. */
    max-width: calc(
      100vw - 48px - var(--dialog-edge-margin) - var(--inset-left) - var(--inset-right)
    );
    max-height: calc(100% - 60px);
    overflow-y: auto;
    background: var(--surface-2);
    border: 1px solid var(--border-strong);
    border-radius: 8px;
    padding: 10px 12px;
    z-index: 6;
    font-size: 12px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.4);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 8px;
    font-weight: 600;
  }

  .close {
    display: inline-flex;
    border: none;
    background: none;
    color: #999;
    cursor: pointer;
    padding: 2px;
  }

  .close:hover {
    color: #fff;
  }

  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 4px 10px;
    margin: 0;
  }

  dt {
    opacity: 0.55;
  }

  dd {
    margin: 0;
    text-align: right;
    word-break: break-word;
  }

  .gps {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 10px;
    padding: 5px 8px;
    width: 100%;
    justify-content: center;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--control);
    color: #8fd0ff;
    font-family: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .gps:hover {
    border-color: var(--accent);
  }

  .path {
    margin: 10px 0 0;
    opacity: 0.5;
    font-size: 11px;
    word-break: break-all;
  }

  .muted {
    opacity: 0.5;
    margin: 4px 0;
  }
</style>
