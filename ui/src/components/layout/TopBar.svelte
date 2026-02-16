<script lang="ts">
  import { getConnectionStatus } from "../../stores/connection.svelte";
  import { getActiveAgent } from "../../stores/agents.svelte";
  import { getToken, setToken, clearToken } from "../../lib/api";

  let { onToggleMetrics, onToggleSidebar, showMetrics }: {
    onToggleMetrics: () => void;
    onToggleSidebar: () => void;
    showMetrics: boolean;
  } = $props();

  let showSettings = $state(false);
  let tokenInput = $state(getToken() ?? "");

  let agent = $derived(getActiveAgent());

  function saveToken() {
    if (tokenInput.trim()) {
      setToken(tokenInput.trim());
      showSettings = false;
      location.reload();
    }
  }

  function logout() {
    clearToken();
    location.reload();
  }
</script>

<header class="topbar">
  <div class="left">
    <button class="hamburger" onclick={onToggleSidebar} aria-label="Toggle sidebar">
      <span class="hamburger-line"></span>
      <span class="hamburger-line"></span>
      <span class="hamburger-line"></span>
    </button>
    <h1 class="title">Aletheia</h1>
    <span class="status-dot" class:connected={getConnectionStatus() === "connected"} class:connecting={getConnectionStatus() === "connecting"}></span>
    {#if agent}
      <span class="active-agent">
        {#if agent.emoji}
          <span class="agent-emoji">{agent.emoji}</span>
        {/if}
        {agent.name}
      </span>
    {/if}
  </div>
  <div class="right">
    <button class="topbar-btn" class:active={showMetrics} onclick={onToggleMetrics}>
      Metrics
    </button>
    <button class="topbar-btn" onclick={() => showSettings = !showSettings}>
      Settings
    </button>
  </div>
</header>

{#if showSettings}
  <div class="settings-panel">
    <label class="settings-label">
      Gateway Token
      <input
        type="password"
        class="settings-input"
        bind:value={tokenInput}
        placeholder="Enter gateway auth token"
      />
    </label>
    <div class="settings-actions">
      <button class="btn-primary" onclick={saveToken}>Save</button>
      <button class="btn-ghost" onclick={logout}>Clear & Logout</button>
      <button class="btn-ghost" onclick={() => showSettings = false}>Cancel</button>
    </div>
  </div>
{/if}

<style>
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--topbar-height);
    padding: 0 16px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
  }
  .left {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .hamburger {
    display: none;
    flex-direction: column;
    gap: 4px;
    background: none;
    border: none;
    padding: 4px;
    cursor: pointer;
  }
  .hamburger-line {
    display: block;
    width: 18px;
    height: 2px;
    background: var(--text-secondary);
    border-radius: 1px;
    transition: background 0.15s;
  }
  .hamburger:hover .hamburger-line {
    background: var(--text);
  }
  .title {
    font-size: 16px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
  }
  .status-dot.connected {
    background: var(--green);
  }
  .status-dot.connecting {
    background: var(--yellow);
    animation: pulse 1.5s ease infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  .active-agent {
    display: none;
    font-size: 13px;
    color: var(--text-secondary);
    font-weight: 500;
  }
  .agent-emoji {
    font-size: 14px;
  }
  .right {
    display: flex;
    gap: 4px;
  }
  .topbar-btn {
    background: none;
    border: 1px solid transparent;
    color: var(--text-secondary);
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    transition: all 0.15s;
  }
  .topbar-btn:hover {
    color: var(--text);
    background: var(--surface);
  }
  .topbar-btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  .topbar-btn.active {
    color: var(--accent);
    border-color: var(--border);
    background: var(--surface);
  }
  .settings-panel {
    padding: 12px 16px;
    background: var(--bg-elevated);
    border-bottom: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    gap: 8px;
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
    padding: 6px 10px;
    font-size: 13px;
    font-family: var(--font-mono);
    width: 100%;
  }
  .settings-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .settings-actions {
    display: flex;
    gap: 8px;
  }
  .btn-primary {
    background: var(--accent);
    border: none;
    color: #fff;
    padding: 6px 14px;
    border-radius: var(--radius-sm);
    font-size: 13px;
    font-weight: 500;
  }
  .btn-primary:hover {
    background: var(--accent-hover);
  }
  .btn-ghost {
    background: none;
    border: 1px solid var(--border);
    color: var(--text-secondary);
    padding: 6px 14px;
    border-radius: var(--radius-sm);
    font-size: 13px;
  }
  .btn-ghost:hover {
    color: var(--text);
    border-color: var(--text-muted);
  }

  @media (max-width: 768px) {
    .hamburger {
      display: flex;
    }
    .title {
      font-size: 14px;
    }
    .active-agent {
      display: inline;
    }
  }
</style>
