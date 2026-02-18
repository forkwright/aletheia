import { fetchHistory } from "../lib/api";
import { streamMessage } from "../lib/stream";
import type { ChatMessage, ToolCallState, HistoryMessage, MediaItem } from "../lib/types";

interface AgentChatState {
  messages: ChatMessage[];
  isStreaming: boolean;
  remoteStreaming: boolean;
  streamingText: string;
  activeToolCalls: ToolCallState[];
  error: string | null;
  abortController: AbortController | null;
}

let states = $state<Record<string, AgentChatState>>({});

const EMPTY: AgentChatState = {
  messages: [],
  isStreaming: false,
  remoteStreaming: false,
  streamingText: "",
  activeToolCalls: [],
  error: null,
  abortController: null,
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
      activeToolCalls: [],
      error: null,
      abortController: null,
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

export function getActiveToolCalls(agentId: string): ToolCallState[] {
  return readState(agentId).activeToolCalls;
}

export function getError(agentId: string): string | null {
  return readState(agentId).error;
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
  state.activeToolCalls = [];
  state.error = null;
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

export async function sendMessage(
  agentId: string,
  text: string,
  sessionKey: string,
  media?: MediaItem[],
): Promise<void> {
  const state = writeState(agentId);
  if (state.isStreaming) return;
  state.error = null;

  // Add user message optimistically
  const userMsg: ChatMessage = {
    id: `user-${Date.now()}`,
    role: "user",
    content: text,
    timestamp: new Date().toISOString(),
    ...(media?.length ? { media } : {}),
  };
  state.messages = [...state.messages, userMsg];

  // Start streaming
  state.isStreaming = true;
  state.streamingText = "";
  state.activeToolCalls = [];
  state.abortController = new AbortController();

  try {
    for await (const event of streamMessage(agentId, text, sessionKey, state.abortController!.signal, media)) {
      switch (event.type) {
        case "text_delta":
          state.streamingText += event.text;
          break;

        case "tool_start":
          state.activeToolCalls = [
            ...state.activeToolCalls,
            { id: event.toolId, name: event.toolName, status: "running" },
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
                }
              : tc,
          );
          break;

        case "turn_complete": {
          const assistantMsg: ChatMessage = {
            id: `assistant-${Date.now()}`,
            role: "assistant",
            content: state.streamingText || event.outcome.text,
            timestamp: new Date().toISOString(),
            toolCalls: state.activeToolCalls.length > 0 ? [...state.activeToolCalls] : undefined,
          };
          state.messages = [...state.messages, assistantMsg];
          state.streamingText = "";
          state.activeToolCalls = [];
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
    // If we still have streaming text (e.g. aborted mid-stream), save it
    if (state.streamingText) {
      const partial: ChatMessage = {
        id: `assistant-${Date.now()}`,
        role: "assistant",
        content: state.streamingText,
        timestamp: new Date().toISOString(),
        toolCalls: state.activeToolCalls.length > 0 ? [...state.activeToolCalls] : undefined,
      };
      state.messages = [...state.messages, partial];
    }
    state.isStreaming = false;
    state.streamingText = "";
    state.activeToolCalls = [];
    state.abortController = null;
  }
}

export function abortStream(agentId: string): void {
  states[agentId]?.abortController?.abort();
}

function historyToMessages(history: HistoryMessage[]): ChatMessage[] {
  const result: ChatMessage[] = [];
  let currentToolCalls: ToolCallState[] = [];

  for (const msg of history) {
    if (msg.role === "user") {
      result.push({
        id: msg.id,
        role: "user",
        content: msg.content,
        timestamp: msg.createdAt,
      });
    } else if (msg.role === "assistant") {
      // Check if it's a JSON content block array (text + tool_use blocks)
      try {
        const parsed = JSON.parse(msg.content);
        if (Array.isArray(parsed) && parsed.length > 0 && parsed[0]?.type) {
          const textBlocks = parsed.filter((b: { type: string }) => b.type === "text");
          const toolBlocks = parsed.filter((b: { type: string }) => b.type === "tool_use");

          if (toolBlocks.length > 0) {
            currentToolCalls = toolBlocks.map((b: { id: string; name: string }) => ({
              id: b.id,
              name: b.name,
              status: "complete" as const,
            }));
          }

          // If there's text alongside tool_use, emit a message with the text
          if (textBlocks.length > 0 && toolBlocks.length > 0) {
            const text = textBlocks.map((b: { text: string }) => b.text).join("\n").trim();
            if (text) {
              result.push({
                id: msg.id,
                role: "assistant",
                content: text,
                timestamp: msg.createdAt,
              });
            }
          }

          // If only tool_use blocks (no text), skip — tool calls attach to next assistant message
          if (toolBlocks.length > 0) continue;

          // If only text blocks (no tool_use), fall through to normal text handling
          if (textBlocks.length > 0) {
            const text = textBlocks.map((b: { text: string }) => b.text).join("\n").trim();
            result.push({
              id: msg.id,
              role: "assistant",
              content: text,
              timestamp: msg.createdAt,
              toolCalls: currentToolCalls.length > 0 ? [...currentToolCalls] : undefined,
            });
            currentToolCalls = [];
            continue;
          }
        }
      } catch {
        // Not JSON, treat as plain text
      }

      result.push({
        id: msg.id,
        role: "assistant",
        content: msg.content,
        timestamp: msg.createdAt,
        toolCalls: currentToolCalls.length > 0 ? [...currentToolCalls] : undefined,
      });
      currentToolCalls = [];
    } else if (msg.role === "tool_result") {
      const tc = currentToolCalls.find((t) => t.id === msg.toolCallId);
      if (tc) {
        tc.result = msg.content.slice(0, 2000);
      }
    }
  }

  return result;
}
