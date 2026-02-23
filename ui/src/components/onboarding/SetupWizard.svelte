<script lang="ts">
  import { createAgent } from "../../lib/api";
  import { getBrandName } from "../../stores/branding.svelte";

  let { onComplete }: { onComplete: () => void } = $props();

  type Step = "credentials" | "agent" | "ready";

  let step = $state<Step>("credentials");
  let credError = $state("");
  let credChecking = $state(false);
  let credFound = $state(false);
  let manualKey = $state("");
  let showManual = $state(false);

  let agentName = $state("");
  let agentEmoji = $state("");
  let agentCreating = $state(false);
  let agentError = $state("");
  let createdName = $state("");
  let createdEmoji = $state("🤖");

  function deriveId(name: string): string {
    return name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 30);
  }

  async function autoDetectCredentials() {
    credChecking = true;
    credError = "";
    try {
      const res = await fetch("/api/setup/credentials", { method: "POST" });
      const data = await res.json() as { success: boolean; error?: string };
      if (data.success) {
        credFound = true;
        step = "agent";
      } else {
        credError = data.error ?? "Auto-detect failed";
        showManual = true;
      }
    } catch {
      credError = "Could not reach server";
      showManual = true;
    } finally {
      credChecking = false;
    }
  }

  async function submitManualKey() {
    if (!manualKey.trim()) return;
    credChecking = true;
    credError = "";
    try {
      const res = await fetch("/api/setup/credentials", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ apiKey: manualKey.trim() }),
      });
      const data = await res.json() as { success: boolean; error?: string };
      if (data.success) {
        credFound = true;
        step = "agent";
      } else {
        credError = data.error ?? "Invalid key";
      }
    } catch {
      credError = "Could not reach server";
    } finally {
      credChecking = false;
    }
  }

  async function createMyAgent() {
    if (!agentName.trim()) return;
    agentCreating = true;
    agentError = "";
    const id = deriveId(agentName);
    const emoji = agentEmoji.trim() || "🤖";
    try {
      await createAgent(id, agentName.trim(), emoji);
      await fetch("/api/setup/complete", { method: "POST" });
      createdName = agentName.trim();
      createdEmoji = emoji;
      step = "ready";
    } catch (err) {
      agentError = err instanceof Error ? err.message : String(err);
    } finally {
      agentCreating = false;
    }
  }

  function handleAgentKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && agentName.trim() && !agentCreating) createMyAgent();
  }

  function handleManualKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && manualKey.trim() && !credChecking) submitManualKey();
  }
</script>

<div class="wizard">
  <div class="wizard-card">
    <div class="progress">
      {#each (["credentials", "agent", "ready"] as Step[]) as s, i}
        <div class="dot" class:active={step === s} class:done={
          (step === "agent" && i === 0) ||
          (step === "ready" && i < 2)
        }></div>
      {/each}
    </div>

    {#if step === "credentials"}
      <h1 class="title">{getBrandName()}</h1>
      <p class="subtitle">Connect your Anthropic account to get started.</p>

      <button class="btn-primary" onclick={autoDetectCredentials} disabled={credChecking}>
        {credChecking ? "Detecting..." : "Auto-detect from Claude Code"}
      </button>

      {#if credError && !showManual}
        <p class="error">{credError}</p>
      {/if}

      {#if showManual}
        <div class="manual-section">
          <p class="manual-label">Enter your API key manually:</p>
          <div class="key-row" onkeydown={handleManualKeydown} role="group">
            <input
              type="password"
              class="key-input"
              placeholder="sk-ant-..."
              bind:value={manualKey}
              disabled={credChecking}
            />
            <button class="btn-secondary" onclick={submitManualKey} disabled={credChecking || !manualKey.trim()}>
              {credChecking ? "..." : "Use key"}
            </button>
          </div>
          {#if credError}
            <p class="error">{credError}</p>
          {/if}
        </div>
      {:else if !credChecking}
        <button class="btn-link" onclick={() => { showManual = true; credError = ""; }}>
          Enter API key manually
        </button>
      {/if}

      <a
        class="key-link"
        href="https://console.anthropic.com/keys"
        target="_blank"
        rel="noopener noreferrer"
      >Get an API key →</a>

    {:else if step === "agent"}
      <h1 class="title">Name your agent</h1>
      <p class="subtitle">Claude will calibrate to your domain and style in your first conversation.</p>

      <div class="agent-form" onkeydown={handleAgentKeydown} role="group">
        <label class="field">
          <span class="field-label">Name</span>
          <input
            type="text"
            class="field-input"
            placeholder="e.g. Chiron"
            bind:value={agentName}
            disabled={agentCreating}
            autofocus
          />
        </label>
        <label class="field">
          <span class="field-label">Emoji (optional)</span>
          <input
            type="text"
            class="field-input emoji-input"
            placeholder="🤖"
            bind:value={agentEmoji}
            disabled={agentCreating}
          />
        </label>
        {#if agentError}
          <p class="error">{agentError}</p>
        {/if}
        <button
          class="btn-primary"
          onclick={createMyAgent}
          disabled={agentCreating || !agentName.trim()}
        >
          {agentCreating ? "Creating..." : "Create agent"}
        </button>
      </div>

    {:else}
      <div class="ready-emoji">{createdEmoji}</div>
      <h1 class="title">{createdName} is ready</h1>
      <p class="subtitle">Your first conversation will calibrate {createdName} to how you work.</p>
      <button class="btn-primary" onclick={onComplete}>Start your first conversation</button>
    {/if}
  </div>
</div>

<style>
  .wizard {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: var(--bg);
  }
  .wizard-card {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 40px 32px;
    max-width: 480px;
    width: 100%;
    text-align: center;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
  }
  .progress {
    display: flex;
    gap: 8px;
    margin-bottom: 8px;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--border);
    transition: background var(--transition-quick);
  }
  .dot.active {
    background: var(--accent);
  }
  .dot.done {
    background: var(--status-success);
  }
  .title {
    font-size: var(--text-2xl);
    font-family: var(--font-display);
    margin: 0;
  }
  .subtitle {
    color: var(--text-secondary);
    font-size: var(--text-base);
    margin: 0;
    max-width: 340px;
  }
  .btn-primary {
    background: var(--accent);
    border: none;
    color: var(--bg);
    padding: 12px 24px;
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
    font-weight: 500;
    width: 100%;
    cursor: pointer;
  }
  .btn-primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }
  .btn-primary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-secondary {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 10px 16px;
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
    white-space: nowrap;
    cursor: pointer;
  }
  .btn-secondary:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .btn-link {
    background: none;
    border: none;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    cursor: pointer;
    text-decoration: underline;
    padding: 0;
  }
  .error {
    color: var(--status-error);
    font-size: var(--text-sm);
    margin: 0;
  }
  .manual-section {
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    text-align: left;
  }
  .manual-label {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    margin: 0;
  }
  .key-row {
    display: flex;
    gap: 8px;
  }
  .key-input {
    flex: 1;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 10px 12px;
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    min-width: 0;
  }
  .key-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .key-link {
    color: var(--text-muted);
    font-size: var(--text-sm);
    text-decoration: none;
  }
  .key-link:hover {
    color: var(--accent);
  }
  .agent-form {
    width: 100%;
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
  .field-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .emoji-input {
    font-size: var(--text-xl);
    max-width: 80px;
  }
  .ready-emoji {
    font-size: 48px;
    line-height: 1;
  }
  @media (max-width: 768px) {
    .wizard-card {
      margin: 0 16px;
      padding: 32px 20px;
    }
  }
</style>
