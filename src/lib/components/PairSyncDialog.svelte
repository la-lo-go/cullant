<script lang="ts">
  import { session } from "../stores/session.svelte";

  let { groupId }: { groupId: number } = $props();
</script>

<div
  class="backdrop"
  onclick={() => (session.recoupleDialogFor = null)}
  onkeydown={(e) => e.key === "Escape" && (session.recoupleDialogFor = null)}
  role="presentation"
>
  <div
    class="dialog"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => e.stopPropagation()}
    role="dialog"
    tabindex="-1"
  >
    <h2>Recouple RAW+JPEG pair</h2>
    <p>
      The two files may have diverged while decoupled. Which state should the
      reunited pair keep?
    </p>
    <div class="options">
      <button onclick={() => session.recouple(groupId, "raw")}>
        Use RAW's state
      </button>
      <button onclick={() => session.recouple(groupId, "jpeg")}>
        Use JPEG's state
      </button>
      <button onclick={() => session.recouple(groupId, "none")}>
        Keep each as-is
      </button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .dialog {
    background: #232329;
    border: 1px solid #3a3a42;
    border-radius: 10px;
    padding: 16px 20px;
    width: 380px;
  }

  h2 {
    margin: 0 0 8px;
    font-size: 15px;
  }

  p {
    font-size: 13px;
    opacity: 0.7;
  }

  .options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 10px;
  }

  button {
    border-radius: 6px;
    border: 1px solid #3a3a42;
    padding: 8px 10px;
    font-size: 13px;
    font-family: inherit;
    color: #e8e8e8;
    background-color: #2a2a30;
    cursor: pointer;
  }

  button:hover {
    border-color: #6b6bff;
  }
</style>
