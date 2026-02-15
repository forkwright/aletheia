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
  import { getActiveAgent, getActiveAgentId } from "../../stores/agents.svelte";
  import {
    getActiveSessionId,
    getActiveSessionKey,
    refreshSessions,
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

  function handleSend(text: string) {
    // Handle slash commands
    if (text.trim() === "/new") {
      clearMessages();
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

  // Tool panel state
  let selectedTools = $state<ToolCallState[] | null>(null);

  function handleToolClick(tools: ToolCallState[]) {
    selectedTools = tools;
  }

  function closeToolPanel() {
    selectedTools = null;
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
