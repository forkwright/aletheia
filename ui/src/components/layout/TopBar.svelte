<script lang="ts">
  import { onMount } from "svelte";
  import { getConnectionStatus } from "../../stores/connection.svelte";
  import { getAgents, getActiveAgent, getActiveAgentId, setActiveAgent } from "../../stores/agents.svelte";
  import { getBrandName } from "../../stores/branding.svelte";
  import { getAccessToken, logout } from "../../lib/auth";
  import { clearToken, getEffectiveToken } from "../../lib/api";
  import { getActiveTurns, getAgentStatus } from "../../lib/events.svelte";
  import { getUnreadCount, markRead } from "../../stores/notifications.svelte";
  import { loadSessions } from "../../stores/sessions.svelte";
  import { getActiveCredentialLabel, getCredentialConfig, loadCredentialConfig } from "../../stores/credentials.svelte";
  import AgentPill from "../agents/AgentPill.svelte";

  type ViewId = "chat" | "metrics" | "graph" | "files" | "settings";

  let { onSetView, activeView }: {
    onSetView: (view: ViewId) => void;
    activeView: ViewId;
  } = $props();

  let agent = $derived(getActiveAgent());
  let hasSession = $derived(!!getAccessToken());
  let showMobileMenu = $state(false);
  let updateAvailable = $state(false);
  let updateVersion = $state("");

  let credLabel = $derived(getActiveCredentialLabel());
  let credConfig = $derived(getCredentialConfig());
  let isBackup = $derived(() => {
    if (!credConfig) return false;
    return credLabel !== credConfig.primary.label;
  });

  function handleAgentClick(id: string) {
    setActiveAgent(id);
    loadSessions(id);
    markRead(id);
    if (activeView !== "chat") onSetView("chat");
  }

  function handleMobileNav(view: ViewId) {
    onSetView(view);
    showMobileMenu = false;
  }

  async function handleLogout() {
    await logout();
    clearToken();
    location.reload();
  }

  onMount(async () => {
    // Load credential configuration
    loadCredentialConfig();

    try {
      const token = getEffectiveToken();
      const res = await fetch("/api/system/update-status", {
        headers: token ? { Authorization: `Bearer ${token}` } : {},
      });
      if (res.ok) {
        const data = await res.json();
        if (data.available) {
          updateAvailable = true;
          updateVersion = data.latest ?? "";
        }
      }
    } catch (e) { console.warn("Update check failed:", e); }
  });
</script>

<header class="topbar">
  <div class="left">
    <h1 class="title desktop-only">{getBrandName()}</h1>
    <span class="status-dot" class:connected={getConnectionStatus() === "connected"} class:connecting={getConnectionStatus() === "connecting"}></span>
    <div class="agent-bar" data-scrollable>
      {#each getAgents() as a (a.id)}
        <AgentPill
          agent={a}
          isActive={a.id === getActiveAgentId()}
          unreadCount={getUnreadCount(a.id)}
          activeTurns={getActiveTurns()[a.id] ?? 0}
          statusLabel={getAgentStatus(a.id)}
          onclick={() => handleAgentClick(a.id)}
        />
      {/each}
      <button class="add-agent-pill" onclick={() => onSetView("settings")} title="Add agent">+</button>
    </div>
    {#if credConfig}
      <span
        class="credential-pill"
        class:is-backup={isBackup()}
        title={isBackup() ? `Using ${credLabel} (failover)` : `Using ${credLabel}`}
      >{credLabel}</span>
    {/if}
    {#if updateAvailable}
      <span class="update-badge" title="Update available: v{updateVersion}">v{updateVersion}</span>
    {/if}
  </div>
  <div class="right desktop-nav">
    <button class="topbar-btn" class:active={activeView === "files"} onclick={() => onSetView(activeView === "files" ? "chat" : "files")}>
      Files
    </button>
    <button class="topbar-btn" class:active={activeView === "metrics"} onclick={() => onSetView(activeView === "metrics" ? "chat" : "metrics")}>
      Metrics
    </button>
    <button class="topbar-btn" class:active={activeView === "graph"} onclick={() => onSetView(activeView === "graph" ? "chat" : "graph")}>
      Graph
    </button>
    <button class="topbar-btn" class:active={activeView === "settings"} onclick={() => onSetView(activeView === "settings" ? "chat" : "settings")}>
      Settings
    </button>
    {#if hasSession}
      <button class="topbar-btn logout-btn" onclick={handleLogout}>
        Logout
      </button>
    {/if}
  </div>
  <div class="right mobile-nav">
    <button
      class="mobile-menu-btn"
      class:active={showMobileMenu}
      onclick={() => showMobileMenu = !showMobileMenu}
      aria-label="Toggle navigation"
    >
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        {#if showMobileMenu}
          <line x1="4" y1="4" x2="16" y2="16" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
          <line x1="16" y1="4" x2="4" y2="16" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
        {:else}
          <circle cx="10" cy="5" r="1.5" fill="currentColor"/>
          <circle cx="10" cy="10" r="1.5" fill="currentColor"/>
          <circle cx="10" cy="15" r="1.5" fill="currentColor"/>
        {/if}
      </svg>
    </button>
  </div>
</header>

{#if showMobileMenu}
  <button class="mobile-menu-overlay" onclick={() => showMobileMenu = false} aria-label="Close menu"></button>
  <div class="mobile-menu">
    <button class="mobile-menu-item" class:active={activeView === "chat"} onclick={() => handleMobileNav("chat")}>
      <span class="mm-icon">💬</span> Chat
    </button>
    <button class="mobile-menu-item" class:active={activeView === "files"} onclick={() => handleMobileNav(activeView === "files" ? "chat" : "files")}>
      <span class="mm-icon">📁</span> Files
    </button>
    <button class="mobile-menu-item" class:active={activeView === "metrics"} onclick={() => handleMobileNav(activeView === "metrics" ? "chat" : "metrics")}>
      <span class="mm-icon">📊</span> Metrics
    </button>
    <button class="mobile-menu-item" class:active={activeView === "graph"} onclick={() => handleMobileNav(activeView === "graph" ? "chat" : "graph")}>
      <span class="mm-icon">🕸️</span> Graph
    </button>
    <button class="mobile-menu-item" class:active={activeView === "settings"} onclick={() => handleMobileNav(activeView === "settings" ? "chat" : "settings")}>
      <span class="mm-icon">⚙️</span> Settings
    </button>
  </div>
{/if}


<style>
  .topbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: var(--topbar-height);
    padding: 0 16px;
    padding-top: var(--safe-top);
    border-bottom: 1px solid var(--border);
    background: var(--bg-elevated);
    flex-shrink: 0;
    position: relative;
    z-index: 100;
  }
  .left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    flex: 1;
    overflow: hidden;
  }
  .agent-bar {
    display: flex;
    align-items: center;
    gap: 2px;
    overflow-x: auto;
    -webkit-overflow-scrolling: touch;
    scrollbar-width: none;
    flex-shrink: 1;
    min-width: 0;
    /* Mark as scrollable so mobile.ts doesn't block touch */
  }
  .agent-bar[data-scrollable] {
    /* Marker attribute for mobile touchmove passthrough */
  }
  .agent-bar::-webkit-scrollbar {
    display: none;
  }
  .add-agent-pill {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    background: none;
    border: 1px dashed var(--border);
    border-radius: var(--radius-pill);
    color: var(--text-muted);
    font-size: var(--text-lg);
    flex-shrink: 0;
    transition: all var(--transition-quick);
  }
  .add-agent-pill:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .title {
    font-family: var(--font-display);
    font-size: var(--text-xl);
    font-weight: 500;
    letter-spacing: 0.01em;
    white-space: nowrap;
  }
  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
    flex-shrink: 0;
  }
  .status-dot.connected {
    background: var(--status-success);
  }
  .status-dot.connecting {
    background: var(--status-warning);
    animation: pulse 1.5s ease infinite;
  }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }
  .credential-pill {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--text-muted);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-pill);
    padding: 2px 8px;
    text-transform: lowercase;
    letter-spacing: 0.02em;
    transition: all var(--transition-quick);
    flex-shrink: 0;
  }
  .credential-pill.is-backup {
    color: var(--status-warning);
    background: color-mix(in srgb, var(--status-warning) 12%, transparent);
    border-color: color-mix(in srgb, var(--status-warning) 30%, transparent);
  }
  .update-badge {
    font-size: var(--text-xs);
    font-family: var(--font-mono);
    color: var(--status-success);
    background: color-mix(in srgb, var(--status-success) 12%, transparent);
    border: 1px solid color-mix(in srgb, var(--status-success) 30%, transparent);
    border-radius: var(--radius);
    padding: 1px 6px;
    cursor: default;
  }
  .right {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }
  .topbar-btn {
    background: none;
    border: 1px solid transparent;
    color: var(--text-secondary);
    padding: 4px 10px;
    border-radius: var(--radius-sm);
    font-size: var(--text-sm);
    transition: all var(--transition-quick);
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
  .logout-btn {
    color: var(--text-muted);
    margin-left: 4px;
  }
  .logout-btn:hover {
    color: var(--status-error);
  }

  /* Mobile menu button */
  .mobile-nav {
    display: none;
  }
  .mobile-menu-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    transition: color var(--transition-quick), background var(--transition-quick);
  }
  .mobile-menu-btn:hover, .mobile-menu-btn.active {
    color: var(--text);
    background: var(--surface);
  }

  /* Mobile dropdown menu */
  .mobile-menu-overlay {
    display: none;
  }
  .mobile-menu {
    display: none;
  }

  @media (max-width: 768px) {
    .topbar {
      padding: 0 12px;
      padding-top: var(--safe-top);
    }
    .desktop-only {
      display: none;
    }
    .desktop-nav {
      display: none;
    }
    .mobile-nav {
      display: flex;
    }
    .mobile-menu-overlay {
      display: block;
      position: fixed;
      inset: 0;
      top: calc(var(--topbar-height) + var(--safe-top));
      background: var(--overlay-mid);
      z-index: 199;
      border: none;
      cursor: default;
    }
    .mobile-menu {
      display: flex;
      flex-direction: column;
      position: fixed;
      top: calc(var(--topbar-height) + var(--safe-top));
      right: 8px;
      z-index: 200;
      background: var(--bg-elevated);
      border: 1px solid var(--border);
      border-radius: var(--radius);
      box-shadow: var(--shadow-lg);
      min-width: 160px;
      overflow: hidden;
      animation: menu-in 0.12s ease;
    }
    @keyframes menu-in {
      from { opacity: 0; transform: translateY(-4px) scale(0.97); }
      to { opacity: 1; transform: translateY(0) scale(1); }
    }
    .mobile-menu-item {
      display: flex;
      align-items: center;
      gap: 10px;
      padding: 14px 16px;
      background: none;
      border: none;
      color: var(--text-secondary);
      font-size: var(--text-base);
      font-weight: 500;
      text-align: left;
      min-height: 48px; /* 48px minimum touch target per Material Design */
      transition: background var(--transition-quick);
    }
    .mobile-menu-item:hover, .mobile-menu-item:active {
      background: var(--surface-hover);
    }
    .mobile-menu-item.active {
      color: var(--accent);
    }
    .mobile-menu-item:not(:last-child) {
      border-bottom: 1px solid var(--border);
    }
    .mm-icon {
      font-size: var(--text-lg);
      width: 24px;
      text-align: center;
    }
  }
</style>
