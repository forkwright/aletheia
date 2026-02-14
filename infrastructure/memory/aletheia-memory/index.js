// Aletheia Memory Plugin — Mem0 integration via lifecycle hooks
// Compatible with clean-room runtime plugin interface (exports hooks object)

const SIDECAR_URL = process.env.ALETHEIA_MEMORY_URL || "http://127.0.0.1:8230";
const USER_ID = process.env.ALETHEIA_MEMORY_USER || "default";
const ADD_TIMEOUT_MS = 60000;
const activeExtractions = new Set();

async function mem0Fetch(path, body, timeoutMs = ADD_TIMEOUT_MS) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(`${SIDECAR_URL}${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

module.exports = {
  hooks: {
    async onAfterTurn(api, result) {
      if (!result.nousId || !result.responseText) return;
      if (result.responseText.length < 100) return;

      const parts = [];
      if (result.messageText) {
        parts.push(`user: ${result.messageText.slice(0, 2000)}`);
      }
      parts.push(`assistant: ${result.responseText.slice(0, 10000)}`);
      const transcript = parts.join("\n");

      const p = mem0Fetch("/add", {
        text: transcript,
        user_id: USER_ID,
        agent_id: result.nousId,
        metadata: {
          source: "after_turn",
          sessionId: result.sessionId,
          timestamp: Date.now(),
        },
      }).finally(() => activeExtractions.delete(p));
      activeExtractions.add(p);
    },

    async onShutdown(api) {
      if (activeExtractions.size > 0) {
        api.log("info", `[aletheia-memory] Waiting for ${activeExtractions.size} active extractions`);
        await Promise.allSettled([...activeExtractions]);
      }
    },

    async onStart(api) {
      try {
        const res = await fetch(`${SIDECAR_URL}/health`, {
          signal: AbortSignal.timeout(3000),
        });
        if (res.ok) {
          api.log("info", "[aletheia-memory] Mem0 sidecar connected");
        } else {
          api.log("warn", `[aletheia-memory] Mem0 sidecar returned ${res.status}`);
        }
      } catch {
        api.log("warn", "[aletheia-memory] Mem0 sidecar unreachable");
      }
    },
  },
};
