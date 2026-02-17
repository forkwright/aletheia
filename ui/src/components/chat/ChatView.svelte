<script lang="ts">
  import MessageList from "./MessageList.svelte";
  import InputBar from "./InputBar.svelte";
  import ToolPanel from "./ToolPanel.svelte";
  import ErrorBanner from "../shared/ErrorBanner.svelte";
  import type { ToolCallState } from "../../lib/types";
  import {
    getMessages,
    getIsStreaming,
    getStreamingText,
    getActiveToolCalls,
    getError,
    clearError,
    sendMessage,
    abortStream,
    loadHistory,
    clearMessages,
  } from "../../stores/chat.svelte";
  import { getActiveAgent, getActiveAgentId, getAgentEmoji } from "../../stores/agents.svelte";
  import {
    getActiveSessionId,
    getActiveSessionKey,
    getActiveSession,
    refreshSessions,
    createNewSession,
  } from "../../stores/sessions.svelte";

  // Load history when active session changes (but skip if we just streamed into it)
  let prevSessionId: string | null = null;
  let skipNextHistoryLoad = false;
  $effect(() => {
    const sessionId = getActiveSessionId();
    if (sessionId && sessionId !== prevSessionId) {
      prevSessionId = sessionId;
      if (skipNextHistoryLoad) {
        skipNextHistoryLoad = false;
      } else {
        loadHistory(sessionId);
      }
    } else if (!sessionId && prevSessionId) {
      prevSessionId = null;
      clearMessages();
    }
  });

  // Slash command registry
  const slashCommands: Record<string, { description: string; handler: () => void }> = {
    "/new": {
      description: "Start a fresh conversation",
      handler: () => {
        const agentId = getActiveAgentId();
        if (agentId) createNewSession(agentId);
        clearMessages();
      },
    },
    "/clear": {
      description: "Clear message display (keeps history)",
      handler: () => clearMessages(),
    },
  };

  function handleSend(text: string) {
    const trimmed = text.trim();

    // Handle slash commands
    const cmd = slashCommands[trimmed];
    if (cmd) {
      cmd.handler();
      return;
    }

    // Show help for unknown slash commands
    if (trimmed.startsWith("/")) {
      // Unknown command — ignore silently
      return;
    }

    const agentId = getActiveAgentId();
    if (!agentId) return;
    const sessionKey = getActiveSessionKey();
    sendMessage(agentId, text, sessionKey).then(() => {
      skipNextHistoryLoad = true;
      refreshSessions(agentId);
    });
  }

  let agent = $derived(getActiveAgent());
  let agentId = $derived(getActiveAgentId());
  let emoji = $derived(agentId ? getAgentEmoji(agentId) : null);

  // Context utilization for distillation indicator
  let session = $derived(getActiveSession());
  let contextPercent = $derived(() => {
    const tokens = session?.tokenCountEstimate ?? 0;
    // 200k context window is the default
    const contextWindow = 200_000;
    return Math.min(100, Math.round((tokens / contextWindow) * 100));
  });

  // Tool panel state
  let selectedTools = $state<ToolCallState[] | null>(null);

  function handleToolClick(tools: ToolCallState[]) {
    selectedTools = tools;
  }

  function closeToolPanel() {
    selectedTools = null;
  }

  function getSlashCommands(): Array<{ command: string; description: string }> {
    return Object.entries(slashCommands).map(([command, { description }]) => ({
      command,
      description,
    }));
  }
</script>

<div class="chat-view">
  {#if getError()}
    <ErrorBanner message={getError()!} onDismiss={clearError} />
  {/if}
  <div class="chat-area">
    <MessageList
      messages={getMessages()}
      streamingText={getStreamingText()}
      activeToolCalls={getActiveToolCalls()}
      isStreaming={getIsStreaming()}
      agentName={agent?.name}
      agentEmoji={emoji}
      onToolClick={handleToolClick}
    />
    {#if selectedTools}
      <ToolPanel tools={selectedTools} onClose={closeToolPanel} />
    {/if}
  </div>
  <InputBar
    isStreaming={getIsStreaming()}
    onSend={handleSend}
    onAbort={abortStream}
    contextPercent={contextPercent()}
    slashCommands={getSlashCommands()}
  />
</div>

<style>
  .chat-view {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }
  .chat-area {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
</style>
