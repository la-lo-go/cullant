<script lang="ts">
  import { catalog } from "../stores/catalog.svelte";
  import { runtimeTools } from "../stores/runtimeTools.svelte";
  import { backdropDismiss } from "../backdrop";
  import { modalFocus } from "../modal";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import X from "@lucide/svelte/icons/x";
  import ExternalLink from "@lucide/svelte/icons/external-link";

  const dismiss = backdropDismiss(() => runtimeTools.dismiss(false));

  $effect(() => {
    const root = catalog.project?.rootPath;
    const ready = !catalog.preloading && catalog.metaProgress.total === 0;
    catalog.generation;
    catalog.mediaCounts.photos;
    catalog.mediaCounts.videos;
    if (root && ready) void runtimeTools.check();
    return () => runtimeTools.cancel();
  });


  function onkeydown(event: KeyboardEvent) {
    event.stopPropagation();
    if (event.key === "Escape") runtimeTools.dismiss(false);
  }
</script>

{#if runtimeTools.pending.length > 0}
  <div class="backdrop" role="presentation" {...dismiss}>
    <div class="dialog" role="dialog" aria-modal="true" aria-label="Missing media tools" tabindex="-1" use:modalFocus {onkeydown}>
      <header>
        <h2>Missing media tools</h2>
        <button class="close" aria-label="Close" onclick={() => runtimeTools.dismiss(false)}><X size={18} /></button>
      </header>
      <div class="body">
        {#each runtimeTools.pending as issue (issue.id)}
          <section>
            <h3>{issue.name}</h3>
            <p>{issue.detail}</p>
            <button class="link" onclick={() => void openUrl(issue.url)}><ExternalLink size={14} />Installation page</button>
            {#if issue.extraLink}
              <button class="link" onclick={() => void openUrl(issue.extraLink!.url)}><ExternalLink size={14} />{issue.extraLink.name}</button>
            {/if}
          </section>
        {/each}
      </div>
      <footer>
        <button data-action="suppress" onclick={() => runtimeTools.dismiss(true)}>Don't show again</button>
        <button class="primary" data-action="ok" onclick={() => runtimeTools.dismiss(false)}>OK</button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop { position: fixed; inset: 0; z-index: 110; display: flex; align-items: center; justify-content: center; background: #0008; padding: var(--dialog-edge-margin); padding-top: calc(var(--inset-top) + var(--dialog-edge-margin)); padding-bottom: calc(var(--inset-bottom) + var(--dialog-edge-margin)); }
  .dialog { outline: none; width: 460px; max-width: 100%; max-height: 100%; display: flex; flex-direction: column; background: #232329; border: 1px solid var(--border-strong); border-radius: 12px; box-shadow: 0 18px 50px #0008; }
  header, footer { display: flex; align-items: center; gap: 12px; padding: 14px 16px; }
  header { border-bottom: 1px solid var(--border); }
  h2 { flex: 1; margin: 0; font-size: 15px; }
  h3 { margin: 0 0 6px; font-size: 13px; }
  .body { overflow-y: auto; padding: 16px; }
  section + section { margin-top: 20px; }
  p { color: #bbb; font-size: 12px; line-height: 1.5; margin: 0 0 8px; }
  button { font: inherit; cursor: pointer; border: 1px solid var(--border); background: var(--control); color: inherit; padding: 7px 12px; border-radius: 6px; font-size: 12px; }
  .close, .link { display: inline-flex; align-items: center; gap: 6px; border: none; background: none; }
  .close { padding: 5px; }
  .link { color: var(--accent); padding: 4px 0; margin-right: 12px; }
  footer { justify-content: flex-end; border-top: 1px solid var(--border); }
  .primary { background: var(--accent-fill); color: #fff; }
</style>
