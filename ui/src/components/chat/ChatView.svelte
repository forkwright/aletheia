<script lang="ts">
  import MessageList from "./MessageList.svelte";
  import InputBar from "./InputBar.svelte";
  import ToolPanel from "./ToolPanel.svelte";
  import ThinkingPanel from "./ThinkingPanel.svelte";
  import ToolApproval from "./ToolApproval.svelte";
  import ErrorBanner from "../shared/ErrorBanner.svelte";
  import type { ToolCallState } from "../../lib/types";
  import {
    getMessages,
    getIsStreaming,
    getStreamingText,
    getThinkingText,
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
    getPendingApproval,
    clearPendingApproval,
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
    setActiveSession,
    refreshSessions,
    createNewSession,
    loadSessions,
  } from "../../stores/sessions.svelte";
  import { distillSession, fetchCommands, executeCommand } from "../../lib/api";
  import type { CommandInfo } from "../../lib/types";
  import { onGlobalEvent } from "../../lib/events";
  import { onMount, onDestroy } from "svelte";
  import { addNotification } from "../../stores/notifications.svelte";
  import { showToast } from "../../stores/toast.svelte";

  let distilling = $state(false);
  let serverCommands = $state<CommandInfo[]>([]);
  let pollInterval: ReturnType<typeof setInterval> | null = null;

  // Recover streaming state after refresh
  let unsubEvents: (() => void) | null = null;

  onMount(() => {
    fetchCommands().then((cmds) => { serverCommands = cmds; }).catch(() => {});

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
        const turnData = data as { nousId?: string; sessionId?: string; text?: string };
        if (turnData.nousId === agentId) {
          setRemoteStreaming(agentId, false);
          if (!hasLocalStream(agentId)) {
            const sessionId = getActiveSessionId();
            if (sessionId) {
              loadHistory(agentId, sessionId);
            }
          }
          refreshSessions(agentId);
        }

        // Notification for non-active agents
        if (turnData.nousId && turnData.nousId !== agentId) {
          const agent = getAgents().find((a) => a.id === turnData.nousId);
          if (agent) {
            const preview = turnData.text?.slice(0, 100) ?? "New message";
            addNotification(turnData.nousId, agent.name, preview);
            showToast(agent.name, agent.emoji, preview, turnData.nousId);
          }
        }
      }

      if (event === "turn:before") {
        const turnData = data as { nousId?: string };
        if (turnData.nousId === agentId) {
          setRemoteStreaming(agentId, true);
        }
      }

      if (event === "connection") {
        const { status } = data as { status: string };
        if (status === "disconnected" && !pollInterval) {
          pollInterval = setInterval(() => {
            const id = getActiveAgentId();
            const sid = getActiveSessionId();
            if (id && sid) loadHistory(id, sid);
          }, 30_000);
        } else if (status === "connected" && pollInterval) {
          clearInterval(pollInterval);
          pollInterval = null;
          const id = getActiveAgentId();
          const sid = getActiveSessionId();
          if (id && sid) loadHistory(id, sid);
        }
      }
    });
  });

  onDestroy(() => {
    unsubEvents?.();
    if (pollInterval) clearInterval(pollInterval);
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
    "/distill": {
      description: "Distill context — compress older messages into long-term memory",
      handler: async () => {
        const id = getActiveAgentId();
        const sessionId = getActiveSessionId();
        if (!id || !sessionId) return;
        if (distilling) return;
        distilling = true;
        injectLocalMessage(id, "*Distilling context...*");
        try {
          await distillSession(sessionId);
          injectLocalMessage(id, "*Context distilled. Older messages compressed into long-term memory.*");
          loadHistory(id, sessionId);
          refreshSessions(id);
        } catch (e) {
          injectLocalMessage(id, `*Distillation failed: ${e instanceof Error ? e.message : String(e)}*`);
        } finally {
          distilling = false;
        }
      },
    },
    "/help": {
      description: "Show available commands",
      handler: () => {
        const id = getActiveAgentId();
        if (!id) return;
        const helpLines = getSlashCommands()
          .map(({ command, description }) => `\`${command}\` — ${description}`)
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

      // Try server command
      const serverCmd = serverCommands.find(
        (sc) => `/${sc.name}` === cmdName || sc.aliases?.some((a) => `/${a}` === cmdName),
      );
      if (serverCmd) {
        const id = getActiveAgentId();
        if (!id) return;
        const sessionId = getActiveSessionId();
        executeCommand(trimmed, sessionId ?? undefined).then((result) => {
          injectLocalMessage(id, result);
        }).catch((err) => {
          injectLocalMessage(id, `*Command failed: ${err instanceof Error ? err.message : String(err)}*`);
        });
        return;
      }

      // Unknown command — ignore
      return;
    }

    const currentAgentId = getActiveAgentId();
    if (!currentAgentId) return;
    const sessionKey = getActiveSessionKey();
    sendMessage(currentAgentId, text, sessionKey, media).then((resolvedSessionId) => {
      // If the server redirected to a different session (e.g., signal key ownership mismatch),
      // switch the UI to the server's session before refreshing the list.
      if (resolvedSessionId && resolvedSessionId !== getActiveSessionId()) {
        setActiveSession(resolvedSessionId);
      }
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

  // Pending tool approval
  let pendingApproval = $derived(currentAgentId ? getPendingApproval(currentAgentId) : null);

  function handleApprovalResolved() {
    if (currentAgentId) clearPendingApproval(currentAgentId);
  }

  // Tool panel state
  let selectedTools = $state<ToolCallState[] | null>(null);

  function handleToolClick(tools: ToolCallState[]) {
    selectedTools = tools;
  }

  function closeToolPanel() {
    selectedTools = null;
  }

  // Thinking panel state
  let selectedThinking = $state<string | null>(null);
  let thinkingIsLive = $state(false);

  function handleThinkingClick(thinking?: string) {
    if (thinking) {
      selectedThinking = thinking;
      thinkingIsLive = false;
    } else {
      selectedThinking = currentAgentId ? getThinkingText(currentAgentId) : "";
      thinkingIsLive = true;
    }
  }

  function closeThinkingPanel() {
    selectedThinking = null;
    thinkingIsLive = false;
  }

  // Thinking panel persistence — capture thinking content when turn completes
  let previouslyLive = false;
  $effect(() => {
    const isLive = thinkingIsLive && currentAgentId ? getIsStreaming(currentAgentId) : false;
    if (previouslyLive && !isLive && currentAgentId) {
      const msgs = getMessages(currentAgentId);
      const lastMsg = msgs[msgs.length - 1];
      if (lastMsg?.thinking) {
        selectedThinking = lastMsg.thinking;
      }
      thinkingIsLive = false;
    }
    previouslyLive = isLive;
  });

  function handleAbort() {
    const id = getActiveAgentId();
    if (id) abortStream(id);
  }

  function getSlashCommands(): Array<{ command: string; description: string }> {
    const clientCmds = Object.entries(slashCommands).map(([command, { description }]) => ({
      command,
      description,
    }));
    const clientNames = new Set(clientCmds.map((c) => c.command.slice(1)));
    const serverCmds = serverCommands
      .filter((sc) => !clientNames.has(sc.name))
      .map((sc) => ({ command: `/${sc.name}`, description: sc.description }));
    return [...clientCmds, ...serverCmds];
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
      thinkingText={currentAgentId ? getThinkingText(currentAgentId) : ""}
      activeToolCalls={currentAgentId ? getActiveToolCalls(currentAgentId) : []}
      isStreaming={currentAgentId ? getIsStreaming(currentAgentId) : false}
      agentName={agent?.name}
      agentEmoji={emoji}
      onToolClick={handleToolClick}
      onThinkingClick={(thinking) => handleThinkingClick(thinking)}
    />
    {#if selectedTools}
      <ToolPanel tools={selectedTools} onClose={closeToolPanel} />
    {/if}
    {#if selectedThinking !== null}
      <ThinkingPanel
        thinkingText={thinkingIsLive && currentAgentId ? getThinkingText(currentAgentId) : selectedThinking}
        isStreaming={thinkingIsLive && (currentAgentId ? getIsStreaming(currentAgentId) : false)}
        onClose={closeThinkingPanel}
      />
    {/if}
  </div>
  {#if pendingApproval}
    <ToolApproval approval={pendingApproval} onResolved={handleApprovalResolved} />
  {/if}
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
