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
    injectLocalMessage,
  } from "../../stores/chat.svelte";
  import {
    getActiveAgent,
    getActiveAgentId,
    getAgentEmoji,
    getAgents,
    setActiveAgent,
  } from "../../stores/agents.svelte";
  import {
    getActiveSessionId,
    getActiveSessionKey,
    getActiveSession,
    refreshSessions,
    createNewSession,
    loadSessions,
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
  const slashCommands: Record<string, { description: string; handler: (args?: string) => void }> = {
    "/new": {
      description: "Start a fresh conversation",
      handler: () => {
        const agentId = getActiveAgentId();
        if (agentId) createNewSession(agentId);
        clearMessages();
      },
    },
    "/clear": {
      description: "Clear message display (keeps server history)",
      handler: () => clearMessages(),
    },
    "/switch": {
      description: "Switch agent — /switch <name>",
      handler: (args?: string) => {
        if (!args) return;
        const name = args.toLowerCase().trim();
        const agent = getAgents().find((a) =>
          a.name.toLowerCase() === name || a.id.toLowerCase() === name,
        );
        if (agent) {
          setActiveAgent(agent.id);
          loadSessions(agent.id);
        }
      },
    },
    "/help": {
      description: "Show available commands",
      handler: () => {
        const helpLines = Object.entries(slashCommands)
          .map(([cmd, { description }]) => `\`${cmd}\` — ${description}`)
          .join("\n");
        injectLocalMessage(`**Available commands:**\n${helpLines}`);
      },
    },
  };

  function handleSend(text: string) {
    const trimmed = text.trim();

    // Handle slash commands
    if (trimmed.startsWith("/")) {
      const spaceIdx = trimmed.indexOf(" ");
      const cmdName = spaceIdx > 0 ? trimmed.slice(0, spaceIdx) : trimmed;
      const args = spaceIdx > 0 ? trimmed.slice(spaceIdx + 1) : undefined;

      const cmd = slashCommands[cmdName];
      if (cmd) {
        cmd.handler(args);
        return;
      }

      // Unknown command — ignore
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
