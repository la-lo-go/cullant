<script lang="ts">
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { open } from "@tauri-apps/plugin-dialog";

  interface ProjectInfo {
    rootPath: string;
    dbPath: string;
    schemaVersion: number;
    fileCount: number;
  }

  let project = $state<ProjectInfo | null>(null);
  let error = $state("");

  // M0 smoke test: an image served through the cullant:// protocol,
  // proving pixels never cross the IPC bridge.
  const testImageUrl = convertFileSrc("test", "cullant");

  async function openProject() {
    error = "";
    try {
      const path = await open({ directory: true, title: "Open project folder" });
      if (!path) return;
      project = await invoke<ProjectInfo>("open_project", { path });
    } catch (e) {
      error = String(e);
    }
  }

  async function closeProject() {
    await invoke("close_project");
    project = null;
  }
</script>

<main class="container">
  <h1>Cullant</h1>
  <p class="tagline">Fast, keyboard-first photo culling</p>

  {#if project}
    <section class="project">
      <h2>Project open</h2>
      <dl>
        <dt>Root</dt>
        <dd>{project.rootPath}</dd>
        <dt>Database</dt>
        <dd>{project.dbPath}</dd>
        <dt>Schema version</dt>
        <dd>{project.schemaVersion}</dd>
        <dt>Indexed files</dt>
        <dd>{project.fileCount}</dd>
      </dl>
      <button onclick={closeProject}>Close project</button>
    </section>
  {:else}
    <button onclick={openProject}>Open project…</button>
  {/if}

  {#if error}
    <p class="error">{error}</p>
  {/if}

  <section class="protocol-test">
    <p>Protocol smoke test (<code>cullant://test</code>):</p>
    <img src={testImageUrl} alt="cullant protocol test" width="64" height="64" />
  </section>
</main>

<style>
  :root {
    font-family: Inter, "Segoe UI", Avenir, Helvetica, Arial, sans-serif;
    font-size: 15px;
    color: #e8e8e8;
    background-color: #1b1b1f;
  }

  .container {
    max-width: 640px;
    margin: 0 auto;
    padding: 3rem 1rem;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 1rem;
    text-align: center;
  }

  h1 {
    margin: 0;
  }

  .tagline {
    margin: 0;
    opacity: 0.7;
  }

  button {
    border-radius: 8px;
    border: 1px solid #3a3a42;
    padding: 0.6em 1.4em;
    font-size: 1em;
    font-weight: 500;
    font-family: inherit;
    color: #e8e8e8;
    background-color: #2a2a30;
    cursor: pointer;
  }

  button:hover {
    border-color: #6b6bff;
  }

  .project dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 0.25rem 1rem;
    text-align: left;
  }

  .project dt {
    opacity: 0.6;
  }

  .project dd {
    margin: 0;
    word-break: break-all;
  }

  .error {
    color: #ff6b6b;
  }

  .protocol-test {
    margin-top: 2rem;
    opacity: 0.8;
  }
</style>
