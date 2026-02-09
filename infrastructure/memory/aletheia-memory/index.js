// Aletheia Memory Plugin — Mem0 integration via lifecycle hooks

const fs = require("node:fs");
const SIDECAR_URL = process.env.ALETHEIA_MEMORY_URL || "http://127.0.0.1:8230";
const USER_ID = process.env.ALETHEIA_MEMORY_USER || "ck";
const MAX_CONTEXT_MEMORIES = 10;
const MAX_TRANSCRIPT_CHARS = 50000;
const SEARCH_TIMEOUT_MS = 8000;
const ADD_TIMEOUT_MS = 60000;

async function mem0Fetch(path, body, timeoutMs = SEARCH_TIMEOUT_MS) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const res = await fetch(`${SIDECAR_URL}${path}`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    if (!res.ok) {
      const text = await res.text().catch(() => "");
      console.error(`mem0 ${path} failed: ${res.status} ${text}`);
      return null;
    }
    return await res.json();
  } catch (err) {
    if (err.name !== "AbortError") {
      console.error(`mem0 ${path} error: ${err.message}`);
    }
    return null;
  } finally {
    clearTimeout(timer);
  }
}

function extractTopicFromPrompt(prompt) {
  if (!prompt || typeof prompt !== "string") return "";
  const lines = prompt.split("\n").filter((l) => l.trim());
  return lines.slice(0, 5).join(" ").slice(0, 500);
}

function buildTranscript(messages) {
  if (!Array.isArray(messages)) return "";
  const parts = [];
  let chars = 0;
  for (const msg of messages) {
    if (chars > MAX_TRANSCRIPT_CHARS) break;
    const role = msg.role || "unknown";
    let content = "";
    if (typeof msg.content === "string") {
      content = msg.content;
    } else if (Array.isArray(msg.content)) {
      content = msg.content
        .filter((b) => b.type === "text")
        .map((b) => b.text)
        .join("\n");
    }
    if (!content) continue;
    const line = `${role}: ${content}`;
    parts.push(line);
    chars += line.length;
  }
  return parts.join("\n\n");
}

function formatMemoriesForContext(results) {
  const memories = results?.results || results || [];
  if (!Array.isArray(memories) || memories.length === 0) return "";
  const lines = ["## Recalled Memories", ""];
  for (const mem of memories) {
    const text = mem.memory || mem.text || "";
    if (text) lines.push(`- ${text}`);
  }
  lines.push("");
  return lines.join("\n");
}

module.exports = {
  id: "aletheia-memory",
  name: "Aletheia Memory",
  version: "1.0.0",

  register(api) {
    try { fs.writeFileSync("/tmp/aletheia-memory-plugin-loaded", new Date().toISOString() + "\n", { flag: "a" }); } catch {}
    console.info("[aletheia-memory] Registering memory hooks");

    api.on(
      "before_agent_start",
      async (event, ctx) => {
        if (!ctx.agentId) return;

        const topic = extractTopicFromPrompt(event.prompt);
        if (!topic) return;

        const result = await mem0Fetch("/search", {
          query: topic,
          user_id: USER_ID,
          agent_id: ctx.agentId,
          limit: MAX_CONTEXT_MEMORIES,
        });

        if (!result?.ok) return;

        const block = formatMemoriesForContext(result.results);
        if (!block) return;

        return { prependContext: block };
      },
      { priority: 5 },
    );

    api.on(
      "agent_end",
      async (event, ctx) => {
        if (!event.success || !ctx.agentId) return;
        if (!event.messages || event.messages.length < 3) return;

        const transcript = buildTranscript(event.messages);
        if (!transcript || transcript.length < 100) return;

        mem0Fetch(
          "/add",
          {
            text: transcript,
            user_id: USER_ID,
            agent_id: ctx.agentId,
            metadata: {
              source: "agent_end",
              sessionKey: ctx.sessionKey || null,
              timestamp: Date.now(),
            },
          },
          ADD_TIMEOUT_MS,
        );
      },
      { priority: 3 },
    );

    api.on(
      "after_compaction",
      async (event, ctx) => {
        if (!ctx.agentId || !ctx.workspaceDir) return;

        const fsp = require("node:fs/promises");
        const path = require("node:path");

        const memDir = path.join(ctx.workspaceDir, "memory");
        try {
          const files = await fsp.readdir(memDir);
          const today = new Date().toISOString().slice(0, 10);
          const todayFile = files.find((f) => f.includes(today) && f.endsWith(".md"));
          if (!todayFile) return;

          const content = await fsp.readFile(path.join(memDir, todayFile), "utf-8");
          if (content.length < 100) return;

          mem0Fetch(
            "/add",
            {
              text: content,
              user_id: USER_ID,
              agent_id: ctx.agentId,
              metadata: {
                source: "compaction_summary",
                sessionKey: ctx.sessionKey || null,
              },
            },
            ADD_TIMEOUT_MS,
          );
        } catch {
          // memory dir may not exist yet
        }
      },
      { priority: 3 },
    );

    api.registerTool({
      name: "mem0_search",
      label: "Long-term Memory Search",
      description:
        "Search long-term extracted memories from past conversations. " +
        "Returns facts, preferences, and entity relationships that were " +
        "automatically captured. Use for cross-session recall, especially " +
        "when memory_search (local files) doesn't have what you need.",
      parameters: {
        type: "object",
        properties: {
          query: {
            type: "string",
            description: "Semantic search query",
          },
          limit: {
            type: "number",
            description: "Max results (default 10)",
          },
        },
        required: ["query"],
      },
      execute: async (_toolCallId, params, ctx) => {
        const query =
          typeof params.query === "string" ? params.query : String(params.query || "");
        const limit =
          typeof params.limit === "number" ? params.limit : 10;
        const agentId = ctx?.agentId || null;

        const result = await mem0Fetch("/search", {
          query,
          user_id: USER_ID,
          agent_id: agentId,
          limit: Math.min(limit, 20),
        });

        if (!result?.ok) {
          const errText = JSON.stringify({ results: [], error: "mem0 sidecar unavailable" }, null, 2);
          return { content: [{ type: "text", text: errText }] };
        }

        const memories = result.results?.results || result.results || [];
        const formatted = memories.map((m) => ({
          memory: m.memory || m.text || "",
          score: m.score || null,
          agent_id: m.agent_id || null,
          created_at: m.created_at || null,
        }));

        const payload = JSON.stringify({ results: formatted, count: formatted.length }, null, 2);
        return { content: [{ type: "text", text: payload }] };
      },
    });

    api.registerHttpRoute({
      path: "/memory/status",
      handler: async (_req, res) => {
        try {
          const health = await mem0Fetch("/health", {}, 5000);
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify(health || { ok: false, error: "sidecar unreachable" }));
        } catch (err) {
          res.writeHead(500, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ ok: false, error: String(err) }));
        }
      },
    });

    console.info("[aletheia-memory] Memory hooks registered");
  },
};
