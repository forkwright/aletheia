<script lang="ts">
  import TopBar from "./TopBar.svelte";
  import Sidebar from "./Sidebar.svelte";
  import ChatView from "../chat/ChatView.svelte";
  import MetricsView from "../metrics/MetricsView.svelte";
  import SettingsView from "../settings/SettingsView.svelte";
  import FileExplorer from "../files/FileExplorer.svelte";
  import { getToken, setToken } from "../../lib/api";
  import { getBrandName, loadBranding } from "../../stores/branding.svelte";

  type ViewId = "chat" | "metrics" | "graph" | "files" | "settings";

  const SIDEBAR_KEY = "aletheia_sidebar_collapsed";

  let activeView = $state<ViewId>("chat");
  // Load branding before auth so login screen shows the right name
  loadBranding();

  let hasToken = $state(!!getToken());
  let tokenValue = $state("");
  let sidebarCollapsed = $state(localStorage.getItem(SIDEBAR_KEY) === "true");

  function handleTokenSubmit(e: Event) {
    e.preventDefault();
    if (tokenValue.trim()) {
      setToken(tokenValue.trim());
      hasToken = true;
      location.reload();
    }
  }

  function toggleSidebar() {
    sidebarCollapsed = !sidebarCollapsed;
    localStorage.setItem(SIDEBAR_KEY, String(sidebarCollapsed));
  }

  function closeSidebar() {
    if (window.innerWidth <= 768) {
      sidebarCollapsed = true;
      localStorage.setItem(SIDEBAR_KEY, String(sidebarCollapsed));
    }
  }
</script>

{#if !hasToken}
  <div class="token-setup">
    <div class="token-card">
      <h1>{getBrandName()}</h1>
      <p>Enter your gateway authentication token to get started.</p>
      <form onsubmit={handleTokenSubmit}>
        <input
          type="password"
          class="token-input"
          placeholder="Gateway token"
          bind:value={tokenValue}
        />
        <button type="submit" class="token-submit">Connect</button>
      </form>
    </div>
  </div>
{:else}
  <TopBar
    onSetView={(v) => activeView = v}
    onToggleSidebar={toggleSidebar}
    {activeView}
    {sidebarCollapsed}
  />
  <div class="main">
    <Sidebar collapsed={sidebarCollapsed} onAgentSelect={closeSidebar} />
    {#if !sidebarCollapsed}
      <button class="sidebar-overlay" onclick={closeSidebar} aria-label="Close sidebar"></button>
    {/if}
    <div class="content">
      {#if activeView === "metrics"}
        <MetricsView />
      {:else if activeView === "graph"}
        {#await import("../graph/GraphView.svelte") then { default: GraphView }}
          <GraphView />
        {:catch}
          <div style="padding:2rem;color:var(--text-secondary)">Failed to load graph view</div>
        {/await}
      {:else if activeView === "files"}
        <FileExplorer />
      {:else if activeView === "settings"}
        <SettingsView />
      {:else}
        <ChatView />
      {/if}
    </div>
  </div>
{/if}

<style>
  .main {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    position: relative;
  }
  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .sidebar-overlay {
    display: none;
  }
  .token-setup {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100vh;
    background: var(--bg);
  }
  .token-card {
    background: var(--bg-elevated);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 32px;
    max-width: 400px;
    width: 100%;
    text-align: center;
  }
  .token-card h1 {
    font-size: 24px;
    margin-bottom: 8px;
  }
  .token-card p {
    color: var(--text-secondary);
    font-size: 14px;
    margin-bottom: 20px;
  }
  .token-card form {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .token-input {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    color: var(--text);
    padding: 10px 14px;
    font-size: 14px;
    font-family: var(--font-mono);
    width: 100%;
  }
  .token-input:focus {
    outline: none;
    border-color: var(--accent);
  }
  .token-submit {
    background: var(--accent);
    border: none;
    color: #fff;
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: 14px;
    font-weight: 500;
  }
  .token-submit:hover {
    background: var(--accent-hover);
  }

  @media (max-width: 768px) {
    .sidebar-overlay {
      display: block;
      position: fixed;
      inset: 0;
      top: var(--topbar-height);
      background: rgba(0, 0, 0, 0.5);
      z-index: 99;
      border: none;
      cursor: default;
    }
    .token-card {
      margin: 0 16px;
    }
  }
</style>
