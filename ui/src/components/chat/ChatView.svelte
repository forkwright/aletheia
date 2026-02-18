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
    hasLocalStream,
    loadHistory,
    clearMessages,
    injectLocalMessage,
    setRemoteStreaming,
  } from "../../stores/chat.svelte";
  import type { MediaItem } from "../../lib/types";
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
  import { onGlobalEvent } from "../../lib/events";
  import { onMount, onDestroy } from "svelte";

  // Recover streaming state after refresh
  let unsubEvents: (() => void) | null = null;

  onMount(() => {
    unsubEvents = onGlobalEvent((event, data) => {
      const agentId = getActiveAgentId();
      if (!agentId) return;

      if (event === "init") {
        const initData = data as { activeTurns?: Record<string, number> };
        const activeTurns = initData.activeTurns ?? {};
        if (activeTurns[agentId] && activeTurns[agentId] > 0) {
          setRemoteStreaming(agentId, true);
        }
      }

      if (event === "turn:after") {
        const turnData = data as { nousId?: string; sessionId?: string };
        if (turnData.nousId === agentId) {
          setRemoteStreaming(agentId, false);
          // Only reload from server if no local stream is managing messages
          if (!hasLocalStream(agentId)) {
            const sessionId = getActiveSessionId();
            if (sessionId) {
              loadHistory(agentId, sessionId);
            }
          }
          refreshSessions(agentId);
        }
      }

      if (event === "turn:before") {
        const turnData = data as { nousId?: string };
        if (turnData.nousId === agentId) {
          setRemoteStreaming(agentId, true);
        }
      }
    });
  });

  onDestroy(() => {
    unsubEvents?.();
  });

  // Load history when active session or agent changes
  let prevSessionId: string | null = null;
  let skipNextHistoryLoad = false;
  $effect(() => {
    const sessionId = getActiveSessionId();
    const currentAgentId = getActiveAgentId();
    if (sessionId && currentAgentId && sessionId !== prevSessionId) {
      prevSessionId = sessionId;
      if (skipNextHistoryLoad) {
        skipNextHistoryLoad = false;
      } else {
        loadHistory(currentAgentId, sessionId);
      }
    } else if (!sessionId && prevSessionId) {
      prevSessionId = null;
      if (currentAgentId) clearMessages(currentAgentId);
    }
  });

  // Slash command registry
  const slashCommands: Record<string, { description: string; handler: (args?: string) => void }> = {
    "/new": {
      description: "Start a fresh conversation",
      handler: () => {
        const id = getActiveAgentId();
        if (id) {
          createNewSession(id);
          clearMessages(id);
        }
      },
    },
    "/clear": {
      description: "Clear message display (keeps server history)",
      handler: () => {
        const id = getActiveAgentId();
        if (id) clearMessages(id);
      },
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
        const id = getActiveAgentId();
        if (!id) return;
        const helpLines = Object.entries(slashCommands)
          .map(([cmd, { description }]) => `\`${cmd}\` — ${description}`)
          .join("\n");
        injectLocalMessage(id, `**Available commands:**\n${helpLines}`);
      },
    },
  };

  function handleSend(text: string, media?: MediaItem[]) {
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

    const currentAgentId = getActiveAgentId();
    if (!currentAgentId) return;
    const sessionKey = getActiveSessionKey();
    sendMessage(currentAgentId, text, sessionKey, media).then(() => {
      skipNextHistoryLoad = true;
      refreshSessions(currentAgentId);
    });
  }

  let agent = $derived(getActiveAgent());
  let currentAgentId = $derived(getActiveAgentId());
  let emoji = $derived(currentAgentId ? getAgentEmoji(currentAgentId) : null);

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

  function handleAbort() {
    const id = getActiveAgentId();
    if (id) abortStream(id);
  }

  function getSlashCommands(): Array<{ command: string; description: string }> {
    return Object.entries(slashCommands).map(([command, { description }]) => ({
      command,
      description,
    }));
  }
</script>

<div class="chat-view">
  {#if currentAgentId && getError(currentAgentId)}
    <ErrorBanner message={getError(currentAgentId)!} onDismiss={() => { if (currentAgentId) clearError(currentAgentId); }} />
  {/if}
  <div class="chat-area">
    <MessageList
      messages={currentAgentId ? getMessages(currentAgentId) : []}
      streamingText={currentAgentId ? getStreamingText(currentAgentId) : ""}
      activeToolCalls={currentAgentId ? getActiveToolCalls(currentAgentId) : []}
      isStreaming={currentAgentId ? getIsStreaming(currentAgentId) : false}
      agentName={agent?.name}
      agentEmoji={emoji}
      onToolClick={handleToolClick}
    />
    {#if selectedTools}
      <ToolPanel tools={selectedTools} onClose={closeToolPanel} />
    {/if}
  </div>
  <InputBar
    isStreaming={currentAgentId ? getIsStreaming(currentAgentId) : false}
    onSend={handleSend}
    onAbort={handleAbort}
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
