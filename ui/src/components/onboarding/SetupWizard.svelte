<script lang="ts">
  import { createAgent, setupAccount, type UserProfile } from "../../lib/api";
  import { getBrandName } from "../../stores/branding.svelte";

  let { onComplete }: { onComplete: () => void } = $props();

  type Step = "credentials" | "account" | "profile" | "agent" | "ready";

  const STEPS: Step[] = ["credentials", "account", "profile", "agent", "ready"];

  let step = $state<Step>("credentials");
  let credError = $state("");
  let credChecking = $state(false);
  let manualKey = $state("");
  let showManual = $state(false);

  let accountUsername = $state("");
  let accountPassword = $state("");
  let accountConfirm = $state("");
  let accountError = $state("");
  let accountSaving = $state(false);

  let profileName = $state("");
  let profileRole = $state("");
  let profileStyle = $state<"direct" | "balanced" | "detailed">("balanced");
  let profileNotes = $state("");

  let agentName = $state("");
  let agentEmoji = $state("");
  let agentCreating = $state(false);
  let agentError = $state("");
  let createdName = $state("");
  let createdEmoji = $state("🤖");

  let accountUsernameEl = $state<HTMLInputElement | null>(null);
  let profileNameEl = $state<HTMLInputElement | null>(null);
  let agentNameEl = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (step === "account") accountUsernameEl?.focus();
    else if (step === "profile") profileNameEl?.focus();
    else if (step === "agent") agentNameEl?.focus();
  });

  function deriveId(name: string): string {
    const id = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "").slice(0, 30);
    return id || "agent";
  }

  function stepIndex(s: Step): number {
    return STEPS.indexOf(s);
  }

  async function autoDetectCredentials() {
    credChecking = true;
    credError = "";
    try {
      const res = await fetch("/api/setup/credentials", { method: "POST" });
      const data = await res.json() as { success: boolean; error?: string };
      if (data.success) {
        step = "account";
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
        step = "account";
      } else {
        credError = data.error ?? "Invalid key";
      }
    } catch {
      credError = "Could not reach server";
    } finally {
      credChecking = false;
    }
  }

  async function submitAccount() {
    if (!accountUsername.trim() || !accountPassword) return;
    if (accountPassword !== accountConfirm) {
      accountError = "Passwords don't match";
      return;
    }
    if (accountPassword.length < 8) {
      accountError = "Password must be at least 8 characters";
      return;
    }
    accountSaving = true;
    accountError = "";
    try {
      await setupAccount(accountUsername.trim(), accountPassword);
      step = "profile";
    } catch (err) {
      accountError = err instanceof Error ? err.message : String(err);
    } finally {
      accountSaving = false;
    }
  }

  async function createMyAgent() {
    if (!agentName.trim()) return;
    agentCreating = true;
    agentError = "";
    const id = deriveId(agentName);
    const emoji = agentEmoji.trim() || "🤖";
    const userProfile: UserProfile | undefined = profileName.trim() ? {
      name: profileName.trim(),
      role: profileRole.trim() || "Operator",
      style: profileStyle,
      ...(profileNotes.trim() ? { notes: profileNotes.trim() } : {}),
      timezone: Intl.DateTimeFormat().resolvedOptions().timeZone,
    } : undefined;
    try {
      await createAgent(id, agentName.trim(), emoji, userProfile);
      const completeRes = await fetch("/api/setup/complete", { method: "POST" });
      if (!completeRes.ok) {
        const data = await completeRes.json().catch(() => ({})) as { error?: string };
        throw new Error(data.error ?? "Failed to finalize setup");
      }
      createdName = agentName.trim();
      createdEmoji = emoji;
      step = "ready";
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      agentError = msg.includes("409") || msg.toLowerCase().includes("already exists")
        ? `An agent named "${agentName.trim()}" already exists — try a different name.`
        : msg;
    } finally {
      agentCreating = false;
    }
  }

  function handleManualKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && manualKey.trim() && !credChecking) submitManualKey();
  }

  function handleAccountKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && accountUsername.trim() && accountPassword && accountConfirm && !accountSaving) submitAccount();
  }

  function handleAgentKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && agentName.trim() && !agentCreating) createMyAgent();
  }
</script>

<div class="wizard">
  <div class="wizard-card">
    <div class="progress">
      {#each STEPS as s, i (s)}
        <div
          class="dot"
          class:active={step === s}
          class:done={stepIndex(step) > i}
        ></div>
      {/each}
    </div>

    {#if step === "credentials"}
      <h1 class="title">{getBrandName()}</h1>
      <p class="subtitle">Connect your Anthropic account to get started.</p>

      <button class="btn-primary" onclick={autoDetectCredentials} disabled={credChecking}>
        {credChecking ? "Detecting..." : "Auto-detect from Claude Code"}
      </button>

      {#if credError && !showManual && !credChecking}
        <p class="error">{credError}</p>
      {/if}

      {#if showManual}
        <div class="manual-section">
          <p class="manual-label">Enter your API key or OAuth token manually:</p>
          <form class="key-row" onsubmit={(e) => { e.preventDefault(); handleManualKeydown({ key: "Enter" } as KeyboardEvent); }}>
            <input
              type="password"
              class="key-input"
              placeholder="sk-ant-..."
              bind:value={manualKey}
              disabled={credChecking}
            />
            <button class="btn-secondary" type="submit" disabled={credChecking || !manualKey.trim()}>
              {credChecking ? "..." : "Use key"}
            </button>
          </form>
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

    {:else if step === "account"}
      <h1 class="title">Create your account</h1>
      <p class="subtitle">Set a username and password to secure your instance.</p>

      <form class="form" onsubmit={(e) => { e.preventDefault(); handleAccountKeydown({ key: "Enter" } as KeyboardEvent); }}>
        <label class="field">
          <span class="field-label">Username</span>
          <input
            type="text"
            class="field-input"
            placeholder="e.g. cody"
            bind:value={accountUsername}
            bind:this={accountUsernameEl}
            disabled={accountSaving}
            autocomplete="username"
          />
        </label>
        <label class="field">
          <span class="field-label">Password</span>
          <input
            type="password"
            class="field-input"
            placeholder="At least 8 characters"
            bind:value={accountPassword}
            disabled={accountSaving}
            autocomplete="new-password"
          />
        </label>
        <label class="field">
          <span class="field-label">Confirm password</span>
          <input
            type="password"
            class="field-input"
            placeholder="Repeat password"
            bind:value={accountConfirm}
            disabled={accountSaving}
            autocomplete="new-password"
          />
        </label>
        {#if accountError}
          <p class="error">{accountError}</p>
        {/if}
        <button
          class="btn-primary"
          type="submit"
          onclick={submitAccount}
          disabled={accountSaving || !accountUsername.trim() || !accountPassword || !accountConfirm}
        >
          {accountSaving ? "Saving..." : "Continue"}
        </button>
      </form>

    {:else if step === "profile"}
      <h1 class="title">About you</h1>
      <p class="subtitle">Help your agent calibrate from the start. You can skip anything.</p>

      <div class="form">
        <label class="field">
          <span class="field-label">Preferred name</span>
          <input
            type="text"
            class="field-input"
            placeholder="How should your agent address you?"
            bind:value={profileName}
            bind:this={profileNameEl}
          />
        </label>
        <label class="field">
          <span class="field-label">Role / title</span>
          <input
            type="text"
            class="field-input"
            placeholder="e.g. Healthcare analytics engineer"
            bind:value={profileRole}
          />
        </label>
        <fieldset class="field style-field">
          <legend class="field-label">Response style</legend>
          <div class="style-options">
            {#each ([
              { value: "direct", label: "Direct", desc: "Answer first, terse, skip preamble" },
              { value: "balanced", label: "Balanced", desc: "Answer first with brief context" },
              { value: "detailed", label: "Detailed", desc: "Full explanations, explore implications" },
            ] as { value: "direct" | "balanced" | "detailed"; label: string; desc: string }[]) as opt (opt.value)}
              <label class="style-option" class:selected={profileStyle === opt.value}>
                <input type="radio" name="style" value={opt.value} bind:group={profileStyle} />
                <span class="style-name">{opt.label}</span>
                <span class="style-desc">{opt.desc}</span>
              </label>
            {/each}
          </div>
        </fieldset>
        <label class="field">
          <span class="field-label">Anything else? <span class="optional">(optional)</span></span>
          <textarea
            class="field-input field-textarea"
            placeholder="Constraints, preferences, or context for your agent..."
            bind:value={profileNotes}
            rows={3}
          ></textarea>
        </label>
        <button class="btn-primary" onclick={() => { step = "agent"; }}>
          Continue
        </button>
        <button class="btn-link" onclick={() => { step = "agent"; }}>Skip for now</button>
      </div>

    {:else if step === "agent"}
      <h1 class="title">Name your agent</h1>
      <p class="subtitle">Give your agent a name and personality.</p>

      <form class="form" onsubmit={(e) => { e.preventDefault(); handleAgentKeydown({ key: "Enter" } as KeyboardEvent); }}>
        <label class="field">
          <span class="field-label">Name</span>
          <input
            type="text"
            class="field-input"
            placeholder="e.g. Chiron"
            bind:value={agentName}
            bind:this={agentNameEl}
            disabled={agentCreating}
          />
        </label>
        <label class="field">
          <span class="field-label">Emoji <span class="optional">(optional)</span></span>
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
          type="submit"
          onclick={createMyAgent}
          disabled={agentCreating || !agentName.trim()}
        >
          {agentCreating ? "Creating..." : "Create agent"}
        </button>
      </form>

    {:else}
      <div class="ready-emoji">{createdEmoji}</div>
      <h1 class="title">{createdName} is ready</h1>
      <p class="subtitle">
        {profileName.trim()
          ? `${createdName} knows who you are. Your first conversation will refine the rest.`
          : `Your first conversation will calibrate ${createdName} to how you work.`}
      </p>
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
  .form {
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
  .optional {
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
  }
  .field-input {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 10px 12px;
    font-size: var(--text-base);
    width: 100%;
    box-sizing: border-box;
  }
  .field-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .field-textarea {
    resize: vertical;
    font-family: inherit;
  }
  .emoji-input {
    font-size: var(--text-xl);
    max-width: 80px;
  }
  .style-field {
    border: none;
    padding: 0;
    margin: 0;
  }
  .style-options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 4px;
  }
  .style-option {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 8px 12px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: border-color var(--transition-quick);
  }
  .style-option input[type="radio"] {
    flex-shrink: 0;
    margin: 0;
    accent-color: var(--accent);
  }
  .style-option.selected {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }
  .style-name {
    font-weight: 600;
    font-size: var(--text-sm);
    white-space: nowrap;
  }
  .style-desc {
    font-size: var(--text-xs);
    color: var(--text-secondary);
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
