import { fetchHistory } from "../lib/api";
import { streamMessage } from "../lib/stream";
import type { ChatMessage, ToolCallState, HistoryMessage, MediaItem, PendingApproval, PlanProposal } from "../lib/types";

interface AgentChatState {
  messages: ChatMessage[];
  isStreaming: boolean;
  remoteStreaming: boolean;
  streamingText: string;
  thinkingText: string;
  activeToolCalls: ToolCallState[];
  error: string | null;
  abortController: AbortController | null;
  pendingApproval: PendingApproval | null;
  pendingPlan: PlanProposal | null;
  // Debounce buffers — accumulate deltas, flush to reactive state on timer
  _textBuffer: string;
  _thinkingBuffer: string;
  _flushTimer: ReturnType<typeof setTimeout> | null;
}

let states = $state<Record<string, AgentChatState>>({});

const STREAM_DEBOUNCE_MS = 100;

const EMPTY: AgentChatState = {
  messages: [],
  isStreaming: false,
  remoteStreaming: false,
  streamingText: "",
  thinkingText: "",
  activeToolCalls: [],
  error: null,
  abortController: null,
  pendingApproval: null,
  pendingPlan: null,
  _textBuffer: "",
  _thinkingBuffer: "",
  _flushTimer: null,
};

// Read-only access — returns default for unknown agents, never mutates during render
function readState(agentId: string): AgentChatState {
  return states[agentId] ?? EMPTY;
}

// Write access — lazily creates state, safe outside render cycle
function writeState(agentId: string): AgentChatState {
  if (!states[agentId]) {
    states[agentId] = {
      messages: [],
      isStreaming: false,
      remoteStreaming: false,
      streamingText: "",
      thinkingText: "",
      activeToolCalls: [],
      error: null,
      abortController: null,
      pendingApproval: null,
      pendingPlan: null,
      _textBuffer: "",
      _thinkingBuffer: "",
      _flushTimer: null,
    };
  }
  return states[agentId]!;
}

export function getMessages(agentId: string): ChatMessage[] {
  return readState(agentId).messages;
}

export function getIsStreaming(agentId: string): boolean {
  const s = readState(agentId);
  return s.isStreaming || s.remoteStreaming;
}

export function setRemoteStreaming(agentId: string, active: boolean): void {
  writeState(agentId).remoteStreaming = active;
}

export function getStreamingText(agentId: string): string {
  return readState(agentId).streamingText;
}

export function getThinkingText(agentId: string): string {
  return readState(agentId).thinkingText;
}

export function getActiveToolCalls(agentId: string): ToolCallState[] {
  return readState(agentId).activeToolCalls;
}

export function getError(agentId: string): string | null {
  return readState(agentId).error;
}

export function getPendingApproval(agentId: string): PendingApproval | null {
  return readState(agentId).pendingApproval;
}

export function clearPendingApproval(agentId: string): void {
  writeState(agentId).pendingApproval = null;
}

export function getPendingPlan(agentId: string): PlanProposal | null {
  return readState(agentId).pendingPlan;
}

export function setPendingPlan(agentId: string, plan: PlanProposal): void {
  writeState(agentId).pendingPlan = plan;
}

export function clearPendingPlan(agentId: string): void {
  writeState(agentId).pendingPlan = null;
}

export function clearError(agentId: string): void {
  writeState(agentId).error = null;
}

export async function loadHistory(agentId: string, sessionId: string): Promise<void> {
  const state = writeState(agentId);
  try {
    const history = await fetchHistory(sessionId);
    state.messages = historyToMessages(history);
  } catch (err) {
    state.error = err instanceof Error ? err.message : String(err);
  }
}

export function clearMessages(agentId: string): void {
  const state = writeState(agentId);
  state.messages = [];
  state.streamingText = "";
  state.thinkingText = "";
  state._textBuffer = "";
  state._thinkingBuffer = "";
  if (state._flushTimer) { clearTimeout(state._flushTimer); state._flushTimer = null; }
  state.activeToolCalls = [];
  state.error = null;
  state.pendingApproval = null;
}

/** Inject a local-only message (not sent to any agent) */
export function injectLocalMessage(agentId: string, content: string): void {
  const state = writeState(agentId);
  const msg: ChatMessage = {
    id: `system-${Date.now()}`,
    role: "assistant",
    content,
    timestamp: new Date().toISOString(),
  };
  state.messages = [...state.messages, msg];
}

/** Flush buffered text/thinking deltas to reactive state immediately */
function flushStreamBuffer(state: AgentChatState): void {
  if (state._flushTimer) {
    clearTimeout(state._flushTimer);
    state._flushTimer = null;
  }
  if (state._textBuffer) {
    state.streamingText += state._textBuffer;
    state._textBuffer = "";
  }
  if (state._thinkingBuffer) {
    state.thinkingText += state._thinkingBuffer;
    state._thinkingBuffer = "";
  }
}

/** Schedule a debounced flush — accumulates deltas, renders at most every STREAM_DEBOUNCE_MS */
function scheduleFlush(state: AgentChatState): void {
  if (!state._flushTimer) {
    state._flushTimer = setTimeout(() => {
      state._flushTimer = null;
      if (state._textBuffer) {
        state.streamingText += state._textBuffer;
        state._textBuffer = "";
      }
      if (state._thinkingBuffer) {
        state.thinkingText += state._thinkingBuffer;
        state._thinkingBuffer = "";
      }
    }, STREAM_DEBOUNCE_MS);
  }
}

export async function sendMessage(
  agentId: string,
  text: string,
  sessionKey: string,
  media?: MediaItem[],
): Promise<string | null> {
  const state = writeState(agentId);
  if (state.isStreaming) return null;
  state.error = null;
  let resolvedSessionId: string | null = null;

  // Add user message optimistically
  const userMsg: ChatMessage = {
    id: `user-${Date.now()}`,
    role: "user",
    content: text,
    timestamp: new Date().toISOString(),
    ...(media?.length ? { media } : {}),
  };
  state.messages = [...state.messages, userMsg];

  // Start streaming — clear buffers and any pending flush
  state.isStreaming = true;
  state.streamingText = "";
  state.thinkingText = "";
  state._textBuffer = "";
  state._thinkingBuffer = "";
  if (state._flushTimer) { clearTimeout(state._flushTimer); state._flushTimer = null; }
  state.activeToolCalls = [];
  state.abortController = new AbortController();

  try {
    for await (const event of streamMessage(agentId, text, sessionKey, state.abortController!.signal, media)) {
      switch (event.type) {
        case "turn_start":
          resolvedSessionId = event.sessionId;
          break;

        case "thinking_delta":
          state._thinkingBuffer += event.text;
          scheduleFlush(state);
          break;

        case "text_delta":
          state._textBuffer += event.text;
          scheduleFlush(state);
          break;

        case "tool_start":
          state.activeToolCalls = [
            ...state.activeToolCalls,
            { id: event.toolId, name: event.toolName, status: "running", input: event.input },
          ];
          break;

        case "tool_result":
          state.activeToolCalls = state.activeToolCalls.map((tc) =>
            tc.id === event.toolId
              ? {
                  ...tc,
                  status: event.isError ? "error" as const : "complete" as const,
                  result: event.result,
                  durationMs: event.durationMs,
                  tokenEstimate: event.tokenEstimate,
                }
              : tc,
          );
          break;

        case "tool_approval_required":
          state.pendingApproval = {
            turnId: event.turnId,
            toolName: event.toolName,
            toolId: event.toolId,
            input: event.input,
            risk: event.risk,
            reason: event.reason,
          };
          break;

        case "tool_approval_resolved":
          state.pendingApproval = null;
          break;

        case "plan_proposed":
          state.pendingPlan = event.plan;
          break;

        case "plan_complete":
          state.pendingPlan = null;
          break;

        case "turn_complete": {
          flushStreamBuffer(state);
          const assistantMsg: ChatMessage = {
            id: `assistant-${Date.now()}`,
            role: "assistant",
            content: state.streamingText || event.outcome.text,
            timestamp: new Date().toISOString(),
            toolCalls: state.activeToolCalls.length > 0 ? [...state.activeToolCalls] : undefined,
            ...(state.thinkingText ? { thinking: state.thinkingText } : {}),
            turnOutcome: event.outcome,
          };
          state.messages = [...state.messages, assistantMsg];
          state.streamingText = "";
          state.thinkingText = "";
          state.activeToolCalls = [];
          state.isStreaming = false;
          break;
        }

        case "turn_abort": {
          flushStreamBuffer(state);
          state.remoteStreaming = false;
          if (state.streamingText) {
            const partial: ChatMessage = {
              id: `assistant-${Date.now()}`,
              role: "assistant",
              content: state.streamingText,
              timestamp: new Date().toISOString(),
              toolCalls: state.activeToolCalls.length > 0 ? [...state.activeToolCalls] : undefined,
            };
            state.messages = [...state.messages, partial];
            state.streamingText = "";
            state.activeToolCalls = [];
          }
          break;
        }

        case "error":
          state.error = event.message;
          break;
      }
    }
  } catch (err) {
    if ((err as Error).name !== "AbortError") {
      state.error = err instanceof Error ? err.message : String(err);
    }
  } finally {
    // Flush any remaining buffered text before saving
    flushStreamBuffer(state);
    // If we still have streaming text (e.g. aborted mid-stream), save it
    if (state.streamingText) {
      const partial: ChatMessage = {
        id: `assistant-${Date.now()}`,
        role: "assistant",
        content: state.streamingText,
        timestamp: new Date().toISOString(),
        toolCalls: state.activeToolCalls.length > 0 ? [...state.activeToolCalls] : undefined,
        ...(state.thinkingText ? { thinking: state.thinkingText } : {}),
      };
      state.messages = [...state.messages, partial];
    }
    state.isStreaming = false;
    state.remoteStreaming = false;
    state.streamingText = "";
    state.thinkingText = "";
    state._textBuffer = "";
    state._thinkingBuffer = "";
    if (state._flushTimer) { clearTimeout(state._flushTimer); state._flushTimer = null; }
    state.activeToolCalls = [];
    state.abortController = null;
    state.pendingApproval = null;
  }
  return resolvedSessionId;
}

export function hasLocalStream(agentId: string): boolean {
  return readState(agentId).abortController !== null;
}

export function abortStream(agentId: string): void {
  const s = states[agentId];
  if (s) {
    s.abortController?.abort();
    s.remoteStreaming = false;
  }
}

function historyToMessages(history: HistoryMessage[]): ChatMessage[] {
  const result: ChatMessage[] = [];
  let pendingToolCalls: ToolCallState[] = [];

  for (const msg of history) {
    if (msg.role === "user") {
      result.push({
        id: msg.id,
        role: "user",
        content: msg.content,
        timestamp: msg.createdAt,
      });
    } else if (msg.role === "assistant") {
      // Try parsing as JSON content block array (text + tool_use + thinking blocks)
      try {
        const parsed = JSON.parse(msg.content);
        if (Array.isArray(parsed) && parsed.length > 0 && parsed[0]?.type) {
          const textBlocks = parsed.filter((b: { type: string }) => b.type === "text");
          const toolBlocks = parsed.filter((b: { type: string }) => b.type === "tool_use");
          const thinkingBlocks = parsed.filter((b: { type: string }) => b.type === "thinking");

          const thinkingText = thinkingBlocks.length > 0
            ? thinkingBlocks.map((b: { thinking: string }) => b.thinking).join("\n\n")
            : undefined;

          // Accumulate tool calls (append, don't overwrite)
          if (toolBlocks.length > 0) {
            pendingToolCalls.push(
              ...toolBlocks.map((b: { id: string; name: string; input?: Record<string, unknown> }) => ({
                id: b.id,
                name: b.name,
                status: "complete" as const,
                input: b.input,
              })),
            );
          }

          // If there's text, emit a message with text + all accumulated tool calls
          if (textBlocks.length > 0) {
            const text = textBlocks.map((b: { text: string }) => b.text).join("\n").trim();
            if (text) {
              result.push({
                id: msg.id,
                role: "assistant",
                content: text,
                timestamp: msg.createdAt,
                toolCalls: pendingToolCalls.length > 0 ? [...pendingToolCalls] : undefined,
                ...(thinkingText ? { thinking: thinkingText } : {}),
              });
              pendingToolCalls = [];
              continue;
            }
          }

          // No text — tool calls or thinking only, skip (tools attach to next text message)
          continue;
        }
      } catch {
        // Not JSON, treat as plain text
      }

      result.push({
        id: msg.id,
        role: "assistant",
        content: msg.content,
        timestamp: msg.createdAt,
        toolCalls: pendingToolCalls.length > 0 ? [...pendingToolCalls] : undefined,
      });
      pendingToolCalls = [];
    } else if (msg.role === "tool_result") {
      const tc = pendingToolCalls.find((t) => t.id === msg.toolCallId);
      if (tc) {
        tc.result = msg.content.slice(0, 2000);
      }
    }
  }

  return result;
}
