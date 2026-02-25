<script lang="ts">
  import { clearToken, createAgent, fetchCredentialInfo, updatePrimaryCredential, addBackupCredential, deleteBackupCredential } from "../../lib/api";
  import type { CredentialInfo } from "../../lib/api";
  import { getAgents, loadAgents, setActiveAgent } from "../../stores/agents.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";
  import { onMount } from "svelte";
  import type { Agent } from "../../lib/types";
  import SessionManager from "./SessionManager.svelte";
  import { fetchAuthMode, getAccessToken, logout as sessionLogout } from "../../lib/auth";

  let { onNavigate }: { onNavigate?: (view: string) => void } = $props();

  const THEME_KEY = "aletheia_theme";
  const FONT_SIZE_KEY = "aletheia_font_size";

  let agents = $state<Agent[]>([]);
  let isSessionAuth = $state(false);
  let theme = $state<"dark" | "light">(
    (localStorage.getItem(THEME_KEY) as "dark" | "light") ?? "dark",
  );
  let fontSize = $state<number>(
    parseInt(localStorage.getItem(FONT_SIZE_KEY) ?? "14", 10),
  );

  let credInfo = $state<CredentialInfo | null>(null);
  let credLoading = $state(false);
  let credError = $state("");

  let primaryType = $state<"oauth" | "api">("api");
  let primaryValue = $state("");
  let primaryLabel = $state("");
  let primarySaving = $state(false);
  let primarySaved = $state(false);

  let backupType = $state<"oauth" | "api">("api");
  let backupValue = $state("");
  let backupLabel = $state("");
  let backupSaving = $state(false);
  let showAddBackup = $state(false);

  async function loadCredentials() {
    credLoading = true;
    credError = "";
    try {
      credInfo = await fetchCredentialInfo();
      primaryLabel = credInfo.primary.label;
      primaryType = credInfo.primary.type === "oauth" ? "oauth" : "api";
    } catch (err) {
      credError = err instanceof Error ? err.message : String(err);
    } finally {
      credLoading = false;
    }
  }

  async function savePrimary() {
    if (!primaryValue.trim()) return;
    primarySaving = true;
    credError = "";
    try {
      await updatePrimaryCredential(primaryType, primaryValue.trim(), primaryLabel.trim() || undefined);
      primaryValue = "";
      primarySaved = true;
      setTimeout(() => { primarySaved = false; }, 3000);
      await loadCredentials();
    } catch (err) {
      credError = err instanceof Error ? err.message : String(err);
    } finally {
      primarySaving = false;
    }
  }

  async function saveBackup() {
    if (!backupValue.trim() || !backupLabel.trim()) return;
    backupSaving = true;
    credError = "";
    try {
      await addBackupCredential(backupType, backupValue.trim(), backupLabel.trim());
      backupValue = "";
      backupLabel = "";
      showAddBackup = false;
      await loadCredentials();
    } catch (err) {
      credError = err instanceof Error ? err.message : String(err);
    } finally {
      backupSaving = false;
    }
  }

  async function removeBackup(label: string) {
    credError = "";
    try {
      await deleteBackupCredential(label);
      await loadCredentials();
    } catch (err) {
      credError = err instanceof Error ? err.message : String(err);
    }
  }

  let showCreateForm = $state(false);
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
      await loadAgents();
      setActiveAgent(formId);
      loadSessions(formId);
      showCreateForm = false;
      formName = "";
      formId = "";
      formEmoji = "";
      onNavigate?.("chat");
    } catch (err) {
      formError = err instanceof Error ? err.message : String(err);
    } finally {
      creating = false;
    }
  }

  onMount(async () => {
    agents = getAgents();
    try {
      const mode = await fetchAuthMode();
      isSessionAuth = mode.sessionAuth;
    } catch {
      // auth mode unavailable
    }
    await loadCredentials();
  });

  async function logout() {
    if (getAccessToken()) {
      await sessionLogout();
    }
    clearToken();
    location.reload();
  }

  function setTheme(t: "dark" | "light") {
    theme = t;
    localStorage.setItem(THEME_KEY, t);
    document.documentElement.setAttribute("data-theme", t);
  }

  function setFontSize(size: number) {
    fontSize = size;
    localStorage.setItem(FONT_SIZE_KEY, String(size));
    document.documentElement.style.fontSize = `${size}px`;
  }
</script>

<div class="settings-view">
  <div class="settings-container">
    <h2 class="settings-heading">Settings</h2>

    <section class="settings-section">
      <h3 class="section-title">Agents</h3>
      {#each agents as agent (agent.id)}
        <div class="setting-row">
          <span class="setting-label">
            {#if agent.emoji}<span class="agent-emoji">{agent.emoji}</span>{/if}
            {agent.name}
          </span>
          <span class="setting-value mono">{agent.model ?? "default"}</span>
        </div>
      {/each}
      {#if agents.length === 0}
        <div class="setting-row">
          <span class="setting-label muted">No agents configured</span>
        </div>
      {/if}
      {#if showCreateForm}
        <div class="create-form">
          <input
            type="text"
            class="settings-input"
            placeholder="Agent name"
            bind:value={formName}
            oninput={handleNameInput}
            disabled={creating}
          />
          <input
            type="text"
            class="settings-input"
            placeholder="ID (auto-derived)"
            bind:value={formId}
            disabled={creating}
          />
          <input
            type="text"
            class="settings-input"
            placeholder="Emoji (optional)"
            bind:value={formEmoji}
            disabled={creating}
          />
          {#if formError}
            <div class="form-error">{formError}</div>
          {/if}
          <div class="form-actions">
            <button class="btn-primary" onclick={handleCreate} disabled={creating || !formName.trim()}>
              {creating ? "Creating..." : "Create"}
            </button>
            <button class="btn-cancel" onclick={() => { showCreateForm = false; formError = ""; }}>
              Cancel
            </button>
          </div>
        </div>
      {:else}
        <button class="btn-add" onclick={() => { showCreateForm = true; }}>+ New Agent</button>
      {/if}
    </section>

    <section class="settings-section">
      <h3 class="section-title">Appearance</h3>
      <div class="setting-row">
        <span class="setting-label">Theme</span>
        <div class="toggle-group">
          <button
            class="toggle-btn"
            class:active={theme === "dark"}
            onclick={() => setTheme("dark")}
          >Dark</button>
          <button
            class="toggle-btn"
            class:active={theme === "light"}
            onclick={() => setTheme("light")}
          >Light</button>
        </div>
      </div>
      <div class="setting-row">
        <span class="setting-label">Font size</span>
        <div class="font-size-control">
          <button class="size-btn" onclick={() => setFontSize(Math.max(11, fontSize - 1))} aria-label="Decrease font size">−</button>
          <span class="size-value">{fontSize}px</span>
          <button class="size-btn" onclick={() => setFontSize(Math.min(20, fontSize + 1))} aria-label="Increase font size">+</button>
        </div>
      </div>
    </section>

    <section class="settings-section">
      <h3 class="section-title">Authentication</h3>
      <div class="auth-actions" style="margin-bottom: 8px">
        <button class="btn-danger" onclick={logout}>Logout</button>
      </div>
      {#if isSessionAuth}
        <SessionManager />
      {/if}
    </section>

    <section class="settings-section">
      <h3 class="section-title">Credentials</h3>

      {#if credError}
        <div class="form-error" style="margin-bottom: 8px">{credError}</div>
      {/if}

      <div class="cred-subsection-label">Primary</div>
      {#if credInfo}
        <div class="setting-row">
          <span class="setting-label">Current</span>
          <span class="setting-value">
            <span class="cred-label mono">{credInfo.primary.label}</span>
            <span class="cred-type-badge cred-type-{credInfo.primary.type}">{credInfo.primary.type}</span>
            {#if credInfo.primary.isExpired}
              <span class="cred-type-badge cred-expired">expired</span>
            {:else if credInfo.primary.expiresInMs !== undefined && credInfo.primary.expiresInMs < 86_400_000}
              <span class="cred-type-badge cred-expiring">expires soon</span>
            {/if}
          </span>
        </div>
      {/if}

      <div class="cred-how-to">
        <div class="how-to-tabs">
          <button
            class="how-to-tab"
            class:active={primaryType === "api"}
            onclick={() => { primaryType = "api"; }}
          >API Key</button>
          <button
            class="how-to-tab"
            class:active={primaryType === "oauth"}
            onclick={() => { primaryType = "oauth"; }}
          >OAuth Token</button>
        </div>
        {#if primaryType === "api"}
          <p class="how-to-text">
            Get an API key from <strong>console.anthropic.com</strong> → API Keys.
            Use this for direct API access or a team billing seat (e.g. a Summus API account).
            Keys start with <code>sk-ant-api03-</code>.
          </p>
        {:else}
          <p class="how-to-text">
            Run <code>claude setup-token</code> in your terminal and paste the result.
            This generates a one-year OAuth token tied to your Claude.ai Max account.
            Tokens start with <code>sk-ant-oat01-</code>.
          </p>
        {/if}
      </div>

      <div class="cred-update-form">
        <div class="cred-row">
          <input
            type="text"
            class="settings-input cred-label-input"
            placeholder="Label (e.g. max-5x)"
            bind:value={primaryLabel}
            disabled={primarySaving}
          />
          <input
            type="password"
            class="settings-input cred-value-input"
            placeholder={primaryType === "oauth" ? "sk-ant-oat01-…" : "sk-ant-api03-…"}
            bind:value={primaryValue}
            disabled={primarySaving}
          />
          <button
            class="btn-primary"
            onclick={savePrimary}
            disabled={primarySaving || !primaryValue.trim()}
          >
            {#if primarySaved}Saved{:else if primarySaving}Saving…{:else}Update{/if}
          </button>
        </div>
      </div>

      <div class="cred-divider"></div>

      <div class="cred-subsection-label">Backups <span class="cred-hint">— used when primary fails or expires</span></div>

      {#if credInfo && credInfo.backups.length > 0}
        {#each credInfo.backups as backup (backup.label)}
          <div class="setting-row">
            <span class="setting-label">
              <span class="mono">{backup.label}</span>
              <span class="cred-type-badge cred-type-{backup.type}">{backup.type}</span>
            </span>
            <button class="btn-remove" onclick={() => removeBackup(backup.label)} aria-label="Remove {backup.label}">Remove</button>
          </div>
        {/each}
      {:else if !credLoading}
        <div class="setting-row">
          <span class="setting-label muted">No backup credentials</span>
        </div>
      {/if}

      {#if showAddBackup}
        <div class="create-form">
          <div class="how-to-tabs">
            <button
              class="how-to-tab"
              class:active={backupType === "api"}
              onclick={() => { backupType = "api"; }}
            >API Key</button>
            <button
              class="how-to-tab"
              class:active={backupType === "oauth"}
              onclick={() => { backupType = "oauth"; }}
            >OAuth Token</button>
          </div>
          <div class="cred-row">
            <input
              type="text"
              class="settings-input cred-label-input"
              placeholder="Label (e.g. api-backup)"
              bind:value={backupLabel}
              disabled={backupSaving}
            />
            <input
              type="password"
              class="settings-input cred-value-input"
              placeholder={backupType === "oauth" ? "sk-ant-oat01-…" : "sk-ant-api03-…"}
              bind:value={backupValue}
              disabled={backupSaving}
            />
          </div>
          <div class="form-actions">
            <button
              class="btn-primary"
              onclick={saveBackup}
              disabled={backupSaving || !backupValue.trim() || !backupLabel.trim()}
            >{backupSaving ? "Saving…" : "Add Backup"}</button>
            <button class="btn-cancel" onclick={() => { showAddBackup = false; backupValue = ""; backupLabel = ""; }}>Cancel</button>
          </div>
        </div>
      {:else}
        <button class="btn-add" onclick={() => { showAddBackup = true; }}>+ Add Backup</button>
      {/if}
    </section>

  </div>
</div>

<style>
  .settings-view {
    height: 100%;
    overflow-y: auto;
    padding: 24px;
    background: var(--bg);
  }
  .settings-container {
    max-width: 640px;
    margin: 0 auto;
  }
  .settings-heading {
    font-size: var(--text-xl);
    font-weight: 600;
    margin-bottom: 24px;
  }
  .settings-section {
    margin-bottom: 24px;
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
  }
  .section-title {
    font-size: var(--text-sm);
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin-bottom: 12px;
  }
  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 6px 0;
    font-size: var(--text-sm);
  }
  .setting-row + .setting-row {
    border-top: 1px solid var(--border);
  }
  .setting-label {
    color: var(--text);
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .setting-label.muted {
    color: var(--text-muted);
  }
  .agent-emoji {
    font-size: var(--text-base);
  }
  .setting-value {
    color: var(--text-secondary);
  }
  .setting-value.mono {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
  }
  .toggle-group {
    display: flex;
    gap: 2px;
    background: var(--surface);
    border-radius: var(--radius-sm);
    padding: 2px;
  }
  .toggle-btn {
    background: none;
    border: none;
    color: var(--text-secondary);
    padding: 4px 12px;
    font-size: var(--text-sm);
    border-radius: var(--radius-sm);
    transition: all var(--transition-quick);
  }
  .toggle-btn.active {
    background: var(--accent);
    color: var(--bg);
  }
  .toggle-btn:not(.active):hover {
    color: var(--text);
  }
  .font-size-control {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .size-btn {
    background: var(--surface);
    border: 1px solid var(--border);
    color: var(--text);
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    font-size: var(--text-lg);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .size-btn:hover {
    background: var(--surface-hover);
  }
  .size-value {
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    color: var(--text-secondary);
    min-width: 36px;
    text-align: center;
  }
  .auth-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .settings-label {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }
  .settings-input {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 8px 10px;
    font-size: var(--text-sm);
    font-family: var(--font-mono);
    width: 100%;
  }
  .settings-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .auth-actions {
    display: flex;
    gap: 8px;
  }
  .btn-primary {
    background: var(--accent);
    border: none;
    color: var(--bg);
    padding: 8px 16px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    font-weight: 500;
  }
  .btn-primary:hover {
    background: var(--accent-hover);
  }
  .btn-danger {
    background: none;
    border: 1px solid var(--status-error);
    color: var(--status-error);
    padding: 8px 16px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
  }
  .btn-danger:hover {
    background: var(--status-error-bg);
  }
  .btn-add {
    width: 100%;
    margin-top: 8px;
    padding: 8px;
    border: 1px dashed var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: var(--text-sm);
    transition: all var(--transition-quick);
  }
  .btn-add:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .create-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--border);
  }
  .form-error {
    color: var(--status-error);
    font-size: var(--text-xs);
  }
  .form-actions {
    display: flex;
    gap: 8px;
  }
  .btn-cancel {
    padding: 8px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-secondary);
    font-size: var(--text-sm);
  }
  .btn-cancel:hover {
    color: var(--text);
  }
  .cred-subsection-label {
    font-size: var(--text-xs);
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
    margin: 4px 0 8px;
  }
  .cred-hint {
    font-weight: 400;
    text-transform: none;
    letter-spacing: 0;
    color: var(--text-muted);
    opacity: 0.7;
  }
  .cred-type-badge {
    font-size: var(--text-xs);
    padding: 1px 6px;
    border-radius: var(--radius-pill);
    font-weight: 600;
    margin-left: 6px;
  }
  .cred-type-oauth { background: var(--status-info-bg, rgba(59,130,246,0.15)); color: var(--status-info, #60a5fa); }
  .cred-type-api   { background: var(--surface); color: var(--text-secondary); border: 1px solid var(--border); }
  .cred-expired    { background: var(--status-error-bg); color: var(--status-error); }
  .cred-expiring   { background: var(--status-warning-bg); color: var(--status-warning); }
  .cred-how-to {
    margin: 10px 0 8px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: 10px 12px;
  }
  .how-to-tabs {
    display: flex;
    gap: 2px;
    margin-bottom: 8px;
  }
  .how-to-tab {
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    padding: 3px 10px;
    font-size: var(--text-xs);
    font-weight: 600;
    transition: all var(--transition-quick);
  }
  .how-to-tab.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--bg);
  }
  .how-to-text {
    font-size: var(--text-xs);
    color: var(--text-secondary);
    line-height: 1.5;
    margin: 0;
  }
  .how-to-text code {
    font-family: var(--font-mono);
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 0 4px;
  }
  .cred-update-form {
    margin-top: 8px;
  }
  .cred-row {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .cred-label-input { flex: 0 0 140px; }
  .cred-value-input { flex: 1; }
  .cred-divider {
    height: 1px;
    background: var(--border);
    margin: 14px 0;
  }
  .btn-remove {
    background: none;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    padding: 3px 10px;
    font-size: var(--text-xs);
    transition: all var(--transition-quick);
  }
  .btn-remove:hover {
    border-color: var(--status-error);
    color: var(--status-error);
  }
</style>
