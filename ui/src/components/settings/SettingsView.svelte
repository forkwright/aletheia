<script lang="ts">
  import { getToken, setToken, clearToken, fetchMetrics, fetchAgents } from "../../lib/api";
  import { getBrandName } from "../../stores/branding.svelte";
  import { getAgents } from "../../stores/agents.svelte";
  import { onMount } from "svelte";
  import type { Agent, MetricsData } from "../../lib/types";

  const THEME_KEY = "aletheia_theme";
  const FONT_SIZE_KEY = "aletheia_font_size";

  let tokenInput = $state(getToken() ?? "");
  let agents = $state<Agent[]>([]);
  let metrics = $state<MetricsData | null>(null);
  let theme = $state<"dark" | "light">(
    (localStorage.getItem(THEME_KEY) as "dark" | "light") ?? "dark",
  );
  let fontSize = $state<number>(
    parseInt(localStorage.getItem(FONT_SIZE_KEY) ?? "14", 10),
  );

  onMount(async () => {
    agents = getAgents();
    try {
      metrics = await fetchMetrics();
    } catch {
      // metrics unavailable
    }
  });

  function saveToken() {
    if (tokenInput.trim()) {
      setToken(tokenInput.trim());
      location.reload();
    }
  }

  function logout() {
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

  function formatUptime(seconds: number): string {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    if (h > 0) return `${h}h ${m}m`;
    return `${m}m`;
  }
</script>

<div class="settings-view">
  <div class="settings-container">
    <h2 class="settings-heading">Settings</h2>

    <section class="settings-section">
      <h3 class="section-title">General</h3>
      <div class="setting-row">
        <span class="setting-label">Instance</span>
        <span class="setting-value">{getBrandName()}</span>
      </div>
      {#if metrics}
        <div class="setting-row">
          <span class="setting-label">Uptime</span>
          <span class="setting-value">{formatUptime(metrics.uptime)}</span>
        </div>
        <div class="setting-row">
          <span class="setting-label">Status</span>
          <span class="setting-value status-ok">{metrics.status}</span>
        </div>
      {/if}
    </section>

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
          <button class="size-btn" onclick={() => setFontSize(Math.max(11, fontSize - 1))}>−</button>
          <span class="size-value">{fontSize}px</span>
          <button class="size-btn" onclick={() => setFontSize(Math.min(20, fontSize + 1))}>+</button>
        </div>
      </div>
    </section>

    <section class="settings-section">
      <h3 class="section-title">Authentication</h3>
      <div class="auth-form">
        <label class="settings-label">
          Gateway Token
          <input
            type="password"
            class="settings-input"
            bind:value={tokenInput}
            placeholder="Enter gateway auth token"
          />
        </label>
        <div class="auth-actions">
          <button class="btn-primary" onclick={saveToken}>Save Token</button>
          <button class="btn-danger" onclick={logout}>Logout</button>
        </div>
      </div>
    </section>

    {#if metrics}
      <section class="settings-section">
        <h3 class="section-title">Usage</h3>
        <div class="setting-row">
          <span class="setting-label">Total turns</span>
          <span class="setting-value mono">{metrics.usage.turnCount.toLocaleString()}</span>
        </div>
        <div class="setting-row">
          <span class="setting-label">Input tokens</span>
          <span class="setting-value mono">{metrics.usage.totalInputTokens.toLocaleString()}</span>
        </div>
        <div class="setting-row">
          <span class="setting-label">Output tokens</span>
          <span class="setting-value mono">{metrics.usage.totalOutputTokens.toLocaleString()}</span>
        </div>
        <div class="setting-row">
          <span class="setting-label">Cache hit rate</span>
          <span class="setting-value mono">{metrics.usage.cacheHitRate}%</span>
        </div>
      </section>

      {#if metrics.services.length > 0}
        <section class="settings-section">
          <h3 class="section-title">Services</h3>
          {#each metrics.services as svc (svc.name)}
            <div class="setting-row">
              <span class="setting-label">{svc.name}</span>
              <span class="setting-value" class:status-ok={svc.healthy} class:status-err={!svc.healthy}>
                {svc.healthy ? "healthy" : svc.message ?? "down"}
              </span>
            </div>
          {/each}
        </section>
      {/if}
    {/if}
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
    font-size: 20px;
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
    font-size: 13px;
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
    font-size: 13px;
  }
  .setting-row + .setting-row {
    border-top: 1px solid rgba(48, 54, 61, 0.4);
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
    font-size: 14px;
  }
  .setting-value {
    color: var(--text-secondary);
  }
  .setting-value.mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .status-ok {
    color: var(--green);
  }
  .status-err {
    color: var(--red);
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
    font-size: 12px;
    border-radius: 4px;
    transition: all 0.15s;
  }
  .toggle-btn.active {
    background: var(--accent);
    color: #fff;
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
    font-size: 16px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .size-btn:hover {
    background: var(--surface-hover);
  }
  .size-value {
    font-family: var(--font-mono);
    font-size: 12px;
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
    font-size: 12px;
    color: var(--text-secondary);
  }
  .settings-input {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 8px 10px;
    font-size: 13px;
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
    color: #fff;
    padding: 8px 16px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    font-weight: 500;
  }
  .btn-primary:hover {
    background: var(--accent-hover);
  }
  .btn-danger {
    background: none;
    border: 1px solid var(--red);
    color: var(--red);
    padding: 8px 16px;
    border-radius: var(--radius-sm);
    font-size: 13px;
  }
  .btn-danger:hover {
    background: rgba(248, 81, 73, 0.1);
  }
</style>
