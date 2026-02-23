import { getEffectiveToken } from "./api";
import type { TurnStreamEvent, MediaItem } from "./types";

const READ_TIMEOUT_MS = 120_000; // 2 min — abort if no data for this long

function readWithTimeout<T>(reader: ReadableStreamDefaultReader<T>, timeoutMs: number): Promise<ReadableStreamReadResult<T>> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      reader.cancel("Read timeout").catch(() => {});
      reject(new Error("Stream read timed out — server may be unreachable"));
    }, timeoutMs);

    reader.read().then(
      (result) => { clearTimeout(timer); resolve(result); },
      (err) => { clearTimeout(timer); reject(err); },
    );
  });
}

export async function* streamMessage(
  agentId: string,
  message: string,
  sessionKey: string,
  signal?: AbortSignal,
  media?: MediaItem[],
): AsyncGenerator<TurnStreamEvent> {
  const base = import.meta.env.DEV ? "" : window.location.origin;
  const token = getEffectiveToken();

  const res = await fetch(`${base}/api/sessions/stream`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
    body: JSON.stringify({
      agentId,
      message,
      sessionKey,
      ...(media?.length ? { media } : {}),
    }),
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
      const { done, value } = await readWithTimeout(reader, READ_TIMEOUT_MS);
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
          } catch (e) {
            console.warn("SSE parse error:", e, "raw:", data);
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
