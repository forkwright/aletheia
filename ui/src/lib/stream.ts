import { getToken } from "./api";
import type { TurnStreamEvent } from "./types";

export async function* streamMessage(
  agentId: string,
  message: string,
  sessionKey: string,
  signal?: AbortSignal,
): AsyncGenerator<TurnStreamEvent> {
  const base = import.meta.env.DEV ? "" : window.location.origin;
  const token = getToken();

  const res = await fetch(`${base}/api/sessions/stream`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({ agentId, message, sessionKey }),
    signal,
  });

  if (!res.ok) {
    const text = await res.text();
    yield { type: "error", message: `HTTP ${res.status}: ${text}` };
    return;
  }

  if (!res.body) {
    yield { type: "error", message: "No response body" };
    return;
  }

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = "";

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });

      // Parse SSE frames from buffer
      const lines = buffer.split("\n");
      buffer = lines.pop() ?? "";

      let eventType: string | null = null;
      let data: string | null = null;

      for (const line of lines) {
        if (line.startsWith("event: ")) {
          eventType = line.slice(7).trim();
        } else if (line.startsWith("data: ")) {
          data = line.slice(6);
        } else if (line === "" && eventType && data) {
          try {
            const parsed = JSON.parse(data) as TurnStreamEvent;
            yield parsed;
          } catch {
            // Skip malformed events
          }
          eventType = null;
          data = null;
        } else if (line.startsWith(":")) {
          // Comment line (ping), skip
        }
      }
    }
  } finally {
    reader.releaseLock();
  }
}
