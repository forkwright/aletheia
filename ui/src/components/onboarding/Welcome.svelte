<script lang="ts">
  import { createAgent } from "../../lib/api";
  import { getBrandName } from "../../stores/branding.svelte";

  let { onComplete }: { onComplete: () => void } = $props();

  let formName = $state("");
  let formId = $state("");
  let formEmoji = $state("");
  let formError = $state("");
  let creating = $state(false);

  function deriveId(name: string): string {
    return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 30);
  }

  function handleNameInput() {
    formId = deriveId(formName);
  }

  async function handleCreate() {
    if (!formName.trim() || !formId.trim()) return;
    creating = true;
    formError = "";
    try {
      await createAgent(formId, formName.trim(), formEmoji.trim() || undefined);
      onComplete();
    } catch (err) {
      formError = err instanceof Error ? err.message : String(err);
    } finally {
      creating = false;
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && formName.trim() && formId.trim() && !creating) {
      handleCreate();
    }
  }
</script>

<div class="welcome">
  <div class="welcome-card">
    <h1 class="welcome-title">{getBrandName()}</h1>
    <p class="welcome-subtitle">Create your first agent to get started.</p>

    <form class="create-form" onsubmit={(e) => { e.preventDefault(); handleKeydown({ key: "Enter" } as KeyboardEvent); }}>
      <label class="field">
        <span class="field-label">Name</span>
        <input
          type="text"
          class="field-input"
          placeholder="e.g. Atlas"
          bind:value={formName}
          oninput={handleNameInput}
          disabled={creating}
        />
      </label>
      <label class="field">
        <span class="field-label">ID</span>
        <input
          type="text"
          class="field-input mono"
          placeholder="auto-derived"
          bind:value={formId}
          disabled={creating}
        />
      </label>
      <label class="field">
        <span class="field-label">Emoji</span>
        <input
          type="text"
          class="field-input"
          placeholder="optional"
          bind:value={formEmoji}
          disabled={creating}
        />
      </label>
      {#if formError}
        <div class="form-error">{formError}</div>
      {/if}
      <button
        class="btn-create"
        type="submit"
        onclick={handleCreate}
        disabled={creating || !formName.trim()}
      >
        {creating ? "Creating..." : "Create Agent"}
      </button>
    </form>

    <p class="welcome-hint">Your agent will guide you through onboarding via conversation.</p>
  </div>
</div>

<style>
  .welcome {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: var(--bg);
  }
  .welcome-card {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 40px 32px;
    max-width: 420px;
    width: 100%;
    text-align: center;
  }
  .welcome-title {
    font-size: var(--text-2xl);
    font-family: var(--font-display);
    margin-bottom: 8px;
  }
  .welcome-subtitle {
    color: var(--text-secondary);
    font-size: var(--text-base);
    margin-bottom: 24px;
  }
  .create-form {
    display: flex;
    flex-direction: column;
    gap: 12px;
    text-align: left;
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .field-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .field-input {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 10px 12px;
    font-size: var(--text-base);
    width: 100%;
  }
  .field-input.mono {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }
  .field-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .form-error {
    color: var(--status-error);
    font-size: var(--text-sm);
  }
  .btn-create {
    margin-top: 4px;
    background: var(--accent);
    border: none;
    color: var(--bg);
    padding: 12px 14px;
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
    font-weight: 500;
  }
  .btn-create:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .btn-create:disabled {
    opacity: 0.5;
  }
  .welcome-hint {
    margin-top: 20px;
    color: var(--text-muted);
    font-size: var(--text-sm);
  }
  @media (max-width: 768px) {
    .welcome-card {
      margin: 0 16px;
      padding: 32px 20px;
    }
  }
</style>
