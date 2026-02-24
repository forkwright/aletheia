<script lang="ts">
  import MessageList from "./MessageList.svelte";
  import InputBar from "./InputBar.svelte";
  import ToolPanel from "./ToolPanel.svelte";
  import ThinkingPanel from "./ThinkingPanel.svelte";
  import PlanningStatusLine from "./PlanningStatusLine.svelte";
  import PlanningDashboard from "./PlanningDashboard.svelte";
  import ToolApproval from "./ToolApproval.svelte";
  import PlanCard from "./PlanCard.svelte";
  import DistillationProgress from "./DistillationProgress.svelte";
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
    getPendingPlan,
    clearPendingPlan,
    addRemoteToolCall,
    setTurnStartedAt,
    getTurnStartedAt,
    injectUserMessage,
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
    isSessionsLoading,
  } from "../../stores/sessions.svelte";
  import { distillSession, fetchCommands, executeCommand, queueMessage } from "../../lib/api";
  import type { CommandInfo } from "../../lib/types";
  import { onGlobalEvent, getActiveTurns } from "../../lib/events.svelte";
  import { onMount, onDestroy, untrack } from "svelte";
  import { addNotification } from "../../stores/notifications.svelte";
  import { showToast } from "../../stores/toast.svelte";

  let distilling = $state(false);
  let serverCommands = $state<CommandInfo[]>([]);
  let pollInterval: ReturnType<typeof setInterval> | null = null;

  // Recover streaming state after refresh
  let unsubEvents: (() => void) | null = null;

  onMount(() => {
    fetchCommands().then((cmds) => { serverCommands = cmds; }).catch(() => {});

    // On mount (including agent switch), sync streaming state from SSE tracker
    const mountAgentId = getActiveAgentId();
    if (mountAgentId) {
      const activeTurns = getActiveTurns();
      if (activeTurns[mountAgentId] && activeTurns[mountAgentId] > 0) {
        setRemoteStreaming(mountAgentId, true);
      }
    }

    unsubEvents = onGlobalEvent((event, data) => {
      // NOTE: Event handlers must track state for ALL agents, not just the
      // currently viewed one. The viewed agent can change at any time via
      // agent pill clicks, and state (remoteStreaming, toolCalls, etc.) must
      // be pre-populated so switching agents shows the right thing immediately.
      const viewedAgent = getActiveAgentId();

      if (event === "init") {
        const initData = data as { activeTurns?: Record<string, number> };
        const activeTurns = initData.activeTurns ?? {};
        // Set remote streaming for ALL agents based on server's authoritative state
        for (const agent of getAgents()) {
          if (activeTurns[agent.id] && activeTurns[agent.id] > 0) {
            setRemoteStreaming(agent.id, true);
          } else {
            setRemoteStreaming(agent.id, false);
          }
        }
        // Reload history for viewed agent on reconnect
        if (viewedAgent) {
          const sid = getActiveSessionId();
          if (sid && !hasLocalStream(viewedAgent)) loadHistory(viewedAgent, sid);
        }
      }

      if (event === "turn:before") {
        const turnData = data as { nousId?: string };
        const nousId = turnData.nousId;
        if (nousId) {
          setRemoteStreaming(nousId, true);
          setTurnStartedAt(nousId, Date.now());
        }
      }

      if (event === "turn:after") {
        const turnData = data as { nousId?: string; sessionId?: string; text?: string };
        const nousId = turnData.nousId;
        if (nousId) {
          setRemoteStreaming(nousId, false);
          setTurnStartedAt(nousId, null);

          // If this is the viewed agent, reload history to show the response
          if (nousId === viewedAgent && !hasLocalStream(nousId)) {
            const sessionId = getActiveSessionId();
            if (sessionId) {
              loadHistory(nousId, sessionId);
            }
            refreshSessions(nousId);
          }

          // Notification for non-viewed agents
          if (nousId !== viewedAgent) {
            const agent = getAgents().find((a) => a.id === nousId);
            if (agent) {
              const preview = turnData.text?.slice(0, 100) ?? "New message";
              addNotification(nousId, agent.name, preview);
              showToast(agent.name, agent.emoji, preview, nousId);
            }
          }
        }
      }

      if (event === "tool:called") {
        const toolData = data as { nousId?: string; tool?: string; durationMs?: number };
        const nousId = toolData.nousId;
        // Track tool calls for ALL agents so switching shows progress
        if (nousId && toolData.tool) {
          addRemoteToolCall(nousId, toolData.tool, toolData.durationMs);
        }
      }

      if (event === "connection") {
        const { status } = data as { status: string };
        if (status === "disconnected") {
          // Clear remote streaming for ALL agents — can't trust state without SSE
          for (const agent of getAgents()) {
            setRemoteStreaming(agent.id, false);
          }
          if (!pollInterval) {
            pollInterval = setInterval(() => {
              const id = getActiveAgentId();
              const sid = getActiveSessionId();
              if (id && sid) loadHistory(id, sid);
            }, 5_000);
          }
        } else if (status === "connected") {
          if (pollInterval) { clearInterval(pollInterval); pollInterval = null; }
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
    } else if (!sessionId && currentAgentId) {
      // Agent active but no session — load sessions once (untrack prevents loop)
      untrack(() => {
        if (!isSessionsLoading()) {
          prevSessionId = null;
          loadSessions(currentAgentId);
        }
      });
    } else if (!sessionId && prevSessionId) {
      prevSessionId = null;
      if (currentAgentId) clearMessages(currentAgentId);
    }
  });

  // Remote streaming state is synced via the onGlobalEvent handler above
  // (turn:before → setRemoteStreaming(true), turn:after → setRemoteStreaming(false))
  // No $effect needed — that would re-fire on every SSE event due to $state object churn.

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

  function handleQueue(text: string) {
    if (!currentAgentId) return;
    injectUserMessage(currentAgentId, text);
    const sessionId = getActiveSessionId();
    if (sessionId) {
      queueMessage(sessionId, text).catch((err) => {
        injectLocalMessage(currentAgentId!, `*Queue failed: ${err instanceof Error ? err.message : String(err)}*`);
      });
    }
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

  let turnStartedAt = $derived(currentAgentId ? getTurnStartedAt(currentAgentId) : null);

  // Pending tool approval
  let pendingApproval = $derived(currentAgentId ? getPendingApproval(currentAgentId) : null);
  let pendingPlan = $derived(currentAgentId ? getPendingPlan(currentAgentId) : null);

  function handleApprovalResolved() {
    if (currentAgentId) clearPendingApproval(currentAgentId);
  }

  function handlePlanResolved() {
    if (currentAgentId) clearPendingPlan(currentAgentId);
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

  // Planning panel state
  let selectedPlanningProjectId = $state<string | null>(null);

  interface ActiveProject {
    id: string;
    state: string;
    activeWave: number | null;
  }

  let activeProject = $state<ActiveProject | null>(null);

  $effect(() => {
    const nousId = currentAgentId;
    if (!nousId) return;

    async function fetchActiveProject(): Promise<void> {
      try {
        const res = await fetch(`/api/planning/projects?nousId=${encodeURIComponent(nousId!)}`, {
          headers: { "Content-Type": "application/json" },
        });
        if (!res.ok) return;
        const data = (await res.json()) as { projects: ActiveProject[] };
        const projects: ActiveProject[] = data.projects ?? [];
        const found = projects.find(
          (p) => p.state !== "complete" && p.state !== "abandoned",
        ) ?? null;
        activeProject = found;
      } catch {
        // best-effort: leave existing activeProject unchanged on transient error
      }
    }

    fetchActiveProject();
    const iv = setInterval(fetchActiveProject, 5000);
    return () => clearInterval(iv);
  });

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
      {turnStartedAt}
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
    {#if selectedPlanningProjectId}
      <PlanningDashboard
        projectId={selectedPlanningProjectId}
        onClose={() => { selectedPlanningProjectId = null; }}
      />
    {/if}
  </div>
  {#if pendingPlan}
    <PlanCard plan={pendingPlan} onResolved={handlePlanResolved} />
  {/if}
  {#if pendingApproval}
    <ToolApproval approval={pendingApproval} onResolved={handleApprovalResolved} />
  {/if}
  {#if activeProject}
    <div class="planning-pill-row">
      <PlanningStatusLine
        projectId={activeProject.id}
        state={activeProject.state}
        activeWave={activeProject.activeWave}
        onclick={() => { selectedPlanningProjectId = activeProject!.id; }}
      />
    </div>
  {/if}
  <DistillationProgress />
  <InputBar
    isStreaming={currentAgentId ? getIsStreaming(currentAgentId) : false}
    onSend={handleSend}
    onAbort={handleAbort}
    onQueue={handleQueue}
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
    /* On mobile with keyboard open, the view must shrink to fit above the keyboard.
       The --app-height variable (set by mobile.ts) handles the outer container,
       and flex layout propagates the constraint inward. */
  }
  .chat-area {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }
  .planning-pill-row {
    padding: 0 8px;
    display: flex;
    align-items: center;
  }

  @media (max-width: 768px) {
    .chat-view {
      /* Ensure the flex column fills available space and doesn't overflow
         when the virtual keyboard is open */
      overflow: hidden;
    }
  }
</style>
