<script lang="ts">
  import TopBar from "./TopBar.svelte";
  import ChatView from "../chat/ChatView.svelte";
  import MetricsView from "../metrics/MetricsView.svelte";
  import SettingsView from "../settings/SettingsView.svelte";
  import FileEditor from "../files/FileEditor.svelte";
  import Login from "../auth/Login.svelte";
  import { getToken, setToken } from "../../lib/api";
  import { fetchAuthMode, getAccessToken, refresh, setAuthFailureHandler, logout } from "../../lib/auth";
  import { getBrandName, loadBranding } from "../../stores/branding.svelte";
  import { getActiveAgentId, isFirstRun, loadAgents } from "../../stores/agents.svelte";
  import Welcome from "../onboarding/Welcome.svelte";
  import Toast from "../shared/Toast.svelte";

  type ViewId = "chat" | "metrics" | "graph" | "settings";
  type AuthState = "loading" | "login" | "token-setup" | "authenticated";

  const FILE_PANEL_WIDTH_KEY = "aletheia_file_panel_width";

  function readLocalStorage(key: string): string | null {
    try { return localStorage.getItem(key); }
    catch { return null; }
  }

  function writeLocalStorage(key: string, value: string): void {
    try { localStorage.setItem(key, value); }
    catch { /* private/incognito mode */ }
  }

  let activeView = $state<ViewId>("chat");
  loadBranding();

  let authState = $state<AuthState>("loading");
  let tokenValue = $state("");
  let hasToken = $state(!!getToken());

  // Determine auth mode on mount
  (async () => {
    try {
      const mode = await fetchAuthMode();
      if (mode.sessionAuth) {
        // Try refreshing an existing session (httpOnly cookie may be valid)
        const ok = await refresh();
        authState = ok ? "authenticated" : "login";
      } else if (mode.mode === "none") {
        authState = "authenticated";
      } else {
        // token mode
        authState = getToken() ? "authenticated" : "token-setup";
      }
    } catch {
      // Can't reach server — fall back to token check
      authState = getToken() ? "authenticated" : "token-setup";
    }
  })();

  // Handle session expiry — redirect to login
  setAuthFailureHandler(() => {
    authState = "login";
  });

  function handleLoginSuccess() {
    authState = "authenticated";
    location.reload();
  }
  let filePanelOpen = $state(false);
  let filePanelWidth = $state(Number(readLocalStorage(FILE_PANEL_WIDTH_KEY)) || 520);
  let resizing = $state(false);

  function handleTokenSubmit(e: Event) {
    e.preventDefault();
    if (tokenValue.trim()) {
      setToken(tokenValue.trim());
      hasToken = true;
      location.reload();
    }
  }

  function toggleFilePanel() {
    filePanelOpen = !filePanelOpen;
  }

  function handleSetView(v: string) {
    if (v === "files") {
      toggleFilePanel();
    } else {
      activeView = v as ViewId;
    }
  }

  function startResize(e: MouseEvent) {
    e.preventDefault();
    resizing = true;
    const startX = e.clientX;
    const startWidth = filePanelWidth;

    function onMouseMove(ev: MouseEvent) {
      const delta = startX - ev.clientX;
      filePanelWidth = Math.max(300, Math.min(startWidth + delta, window.innerWidth - 400));
    }

    function onMouseUp() {
      resizing = false;
      writeLocalStorage(FILE_PANEL_WIDTH_KEY, String(filePanelWidth));
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    }

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  }
</script>

{#if authState === "loading"}
  <div class="token-setup">
    <div class="token-card">
      <h1>{getBrandName()}</h1>
      <p style="color: var(--text-muted)">Connecting...</p>
    </div>
  </div>
{:else if authState === "login"}
  <Login onSuccess={handleLoginSuccess} />
{:else if authState === "token-setup"}
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
  {#if isFirstRun()}
    <Welcome onComplete={() => { loadAgents(); }} />
  {:else}
  <TopBar
    onSetView={handleSetView}
    activeView={filePanelOpen ? "files" : activeView}
  />
  <div class="main">
    <div class="content" class:resizing>
      {#if activeView === "metrics"}
        <MetricsView />
      {:else if activeView === "graph"}
        {#await import("../graph/GraphView.svelte") then { default: GraphView }}
          <GraphView />
        {:catch}
          <div style="padding:2rem;color:var(--text-secondary)">Failed to load graph view</div>
        {/await}
      {:else if activeView === "settings"}
        <SettingsView onNavigate={handleSetView} />
      {:else}
        <div class="chat-pane">
          {#key getActiveAgentId()}
            <ChatView />
          {/key}
        </div>
        {#if filePanelOpen}
          <div
            class="resize-handle"
            onmousedown={startResize}
            role="separator"
            aria-orientation="vertical"
            tabindex="-1"
          ></div>
          <div class="file-pane" style="width: {filePanelWidth}px">
            <FileEditor onClose={toggleFilePanel} />
          </div>
        {/if}
      {/if}
    </div>
  </div>
  <Toast />
  {/if}
{/if}

<style>
  .main {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
    position: relative;
    /* Critical for mobile keyboard: flex child must be able to shrink
       when --app-height decreases */
  }
  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  /* When chat + file panel are shown, switch to row layout */
  .content:has(.chat-pane) {
    flex-direction: row;
  }
  .content.resizing {
    user-select: none;
    cursor: col-resize;
  }
  .chat-pane {
    flex: 1;
    min-width: 300px;
    display: flex;
    flex-direction: column;
  }
  .file-pane {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    border-left: 1px solid var(--border);
    min-height: 0;
  }
  .resize-handle {
    width: 4px;
    cursor: col-resize;
    background: transparent;
    flex-shrink: 0;
    transition: background var(--transition-quick);
  }
  .resize-handle:hover, .resizing .resize-handle {
    background: var(--accent);
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
    font-size: var(--text-2xl);
    margin-bottom: 8px;
  }
  .token-card p {
    color: var(--text-secondary);
    font-size: var(--text-base);
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
    font-size: var(--text-base);
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
    color: var(--bg);
    padding: 10px 14px;
    border-radius: var(--radius-sm);
    font-size: var(--text-base);
    font-weight: 500;
  }
  .token-submit:hover {
    background: var(--accent-hover);
  }

  @media (max-width: 768px) {
    .token-card {
      margin: 0 16px;
    }
    .file-pane {
      display: none;
    }
    .resize-handle {
      display: none;
    }
  }
</style>
