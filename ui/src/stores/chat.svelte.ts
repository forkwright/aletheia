import { fetchHistory } from "../lib/api";
import { streamMessage } from "../lib/stream";
import type { ChatMessage, ToolCallState, HistoryMessage } from "../lib/types";

let messages = $state<ChatMessage[]>([]);
let isStreaming = $state(false);
let streamingText = $state("");
let activeToolCalls = $state<ToolCallState[]>([]);
let error = $state<string | null>(null);
let abortController: AbortController | null = null;

export function getMessages(): ChatMessage[] {
  return messages;
}

export function getIsStreaming(): boolean {
  return isStreaming;
}

export function getStreamingText(): string {
  return streamingText;
}

export function getActiveToolCalls(): ToolCallState[] {
  return activeToolCalls;
}

export function getError(): string | null {
  return error;
}

export function clearError(): void {
  error = null;
}

export async function loadHistory(sessionId: string): Promise<void> {
  try {
    const history = await fetchHistory(sessionId);
    messages = historyToMessages(history);
  } catch (err) {
    error = err instanceof Error ? err.message : String(err);
  }
}

export function clearMessages(): void {
  messages = [];
  streamingText = "";
  activeToolCalls = [];
  error = null;
}

/** Inject a local-only message (not sent to any agent) */
export function injectLocalMessage(content: string): void {
  const msg: ChatMessage = {
    id: `system-${Date.now()}`,
    role: "assistant",
    content,
    timestamp: new Date().toISOString(),
  };
  messages = [...messages, msg];
}

export async function sendMessage(
  agentId: string,
  text: string,
  sessionKey: string,
): Promise<void> {
  if (isStreaming) return;
  error = null;

  // Add user message optimistically
  const userMsg: ChatMessage = {
    id: `user-${Date.now()}`,
    role: "user",
    content: text,
    timestamp: new Date().toISOString(),
  };
  messages = [...messages, userMsg];

  // Start streaming
  isStreaming = true;
  streamingText = "";
  activeToolCalls = [];
  abortController = new AbortController();

  try {
    for await (const event of streamMessage(agentId, text, sessionKey, abortController.signal)) {
      switch (event.type) {
        case "text_delta":
          streamingText += event.text;
          break;

        case "tool_start":
          activeToolCalls = [
            ...activeToolCalls,
            { id: event.toolId, name: event.toolName, status: "running" },
          ];
          break;

        case "tool_result":
          activeToolCalls = activeToolCalls.map((tc) =>
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
            content: streamingText || event.outcome.text,
            timestamp: new Date().toISOString(),
            toolCalls: activeToolCalls.length > 0 ? [...activeToolCalls] : undefined,
          };
          messages = [...messages, assistantMsg];
          streamingText = "";
          activeToolCalls = [];
          break;
        }

        case "error":
          error = event.message;
          break;
      }
    }
  } catch (err) {
    if ((err as Error).name !== "AbortError") {
      error = err instanceof Error ? err.message : String(err);
    }
  } finally {
    // If we still have streaming text (e.g. aborted mid-stream), save it
    if (streamingText) {
      const partial: ChatMessage = {
        id: `assistant-${Date.now()}`,
        role: "assistant",
        content: streamingText,
        timestamp: new Date().toISOString(),
        toolCalls: activeToolCalls.length > 0 ? [...activeToolCalls] : undefined,
      };
      messages = [...messages, partial];
    }
    isStreaming = false;
    streamingText = "";
    activeToolCalls = [];
    abortController = null;
  }
}

export function abortStream(): void {
  abortController?.abort();
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
