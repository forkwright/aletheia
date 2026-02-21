<script lang="ts">
  import { getConnectionStatus } from "../../stores/connection.svelte";
  import { getActiveAgent, getActiveAgentId } from "../../stores/agents.svelte";
  import { getBrandName } from "../../stores/branding.svelte";
  import { getAccessToken, logout } from "../../lib/auth";
  import { clearToken } from "../../lib/api";
  import { getMessages } from "../../stores/chat.svelte";
  import { formatCost, calculateMessageCost } from "../../lib/format";

  type ViewId = "chat" | "metrics" | "graph" | "files" | "settings";

  let { onSetView, onToggleSidebar, activeView, sidebarCollapsed = false }: {
    onSetView: (view: ViewId) => void;
    onToggleSidebar: () => void;
    activeView: ViewId;
    sidebarCollapsed?: boolean;
  } = $props();

  let agent = $derived(getActiveAgent());
  let hasSession = $derived(!!getAccessToken());
  let showMobileMenu = $state(false);

  let sessionCost = $derived(() => {
    const agentId = getActiveAgentId();
    if (!agentId) return 0;
    const msgs = getMessages(agentId);
    let total = 0;
    for (const m of msgs) {
      if (m.turnOutcome) total += calculateMessageCost(m.turnOutcome);
    }
    return total;
  });

  function handleMobileNav(view: ViewId) {
    onSetView(view);
    showMobileMenu = false;
  }

  let sessionCost = $derived(() => {
    const agentId = getActiveAgentId();
    if (!agentId) return 0;
    const msgs = getMessages(agentId);
    let total = 0;
    for (const m of msgs) {
      if (m.turnOutcome) total += calculateMessageCost(m.turnOutcome);
    }
    return total;
  });

  async function handleLogout() {
    await logout();
    clearToken();
    location.reload();
  }
</script>

<header class="topbar">
  <div class="left">
    <button
      class="sidebar-toggle"
      class:open={!sidebarCollapsed}
      onclick={onToggleSidebar}
      aria-label="Toggle sidebar"
      title={sidebarCollapsed ? "Show agents" : "Hide agents"}
    >
      <svg width="18" height="18" viewBox="0 0 18 18" fill="none">
        <rect x="1" y="2" width="16" height="14" rx="2" stroke="currentColor" stroke-width="1.5" fill="none"/>
        <line x1="6.5" y1="2" x2="6.5" y2="16" stroke="currentColor" stroke-width="1.5"/>
      </svg>
    </button>
    <h1 class="title desktop-only">{getBrandName()}</h1>
    <span class="status-dot" class:connected={getConnectionStatus() === "connected"} class:connecting={getConnectionStatus() === "connecting"}></span>
    {#if agent}
      <span class="active-agent">
        {#if agent.emoji}
          <span class="agent-emoji">{agent.emoji}</span>
        {/if}
        <span class="agent-name">{agent.name}</span>
      </span>
    {/if}
    {#if sessionCost() > 0}
      <span class="session-cost" title="Running session cost">{formatCost(sessionCost())}</span>
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
  }
  .left {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .sidebar-toggle {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    background: none;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    color: var(--text-muted);
    transition: color 0.15s, background 0.15s, border-color 0.15s;
    flex-shrink: 0;
  }
  .sidebar-toggle:hover {
    color: var(--text);
    background: var(--surface);
  }
  .sidebar-toggle.open {
    color: var(--text-secondary);
  }
  .title {
    font-size: 16px;
    font-weight: 600;
    letter-spacing: -0.02em;
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
    font-size: 13px;
    color: var(--text-secondary);
    font-weight: 500;
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }
  .agent-emoji {
    font-size: 14px;
    flex-shrink: 0;
  }
  .agent-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .session-cost {
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--text-muted);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 1px 6px;
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
  .logout-btn {
    color: var(--text-muted);
    margin-left: 4px;
  }
  .logout-btn:hover {
    color: var(--red);
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
    transition: color 0.15s, background 0.15s;
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
      background: rgba(0, 0, 0, 0.4);
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
      box-shadow: 0 8px 24px rgba(0, 0, 0, 0.4);
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
      padding: 12px 16px;
      background: none;
      border: none;
      color: var(--text-secondary);
      font-size: 14px;
      font-weight: 500;
      text-align: left;
      transition: background 0.1s;
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
      font-size: 16px;
      width: 24px;
      text-align: center;
    }
  }
</style>
