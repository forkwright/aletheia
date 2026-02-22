// Export and analytics routes
import { Hono } from "hono";
import { createCipheriv, randomBytes, scryptSync } from "node:crypto";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function exportRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { store } = deps;

  app.get("/api/export/stats", (c) => {
    const nousId = c.req.query("nousId");
    const since = c.req.query("since");
    const stats = store.getExportStats({
      ...(nousId ? { nousId } : {}),
      ...(since ? { since } : {}),
    });
    return c.json(stats);
  });

  app.get("/api/export/sessions", (c) => {
    const nousId = c.req.query("nousId");
    const since = c.req.query("since");
    const until = c.req.query("until");
    const sessions = store.listSessionsFiltered({
      ...(nousId ? { nousId } : {}),
      ...(since ? { since } : {}),
      ...(until ? { until } : {}),
    });
    return c.json({ sessions });
  });

  app.get("/api/export/sessions/:id", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    const messages = store.getHistory(id, { excludeDistilled: false });
    const usage = store.getUsageForSession(id);

    const lines: string[] = [];
    lines.push(JSON.stringify({ type: "session", ...session }));
    for (const m of messages) {
      lines.push(JSON.stringify({ type: "message", seq: m.seq, role: m.role, content: m.content, isDistilled: m.isDistilled, toolName: m.toolName ?? null, tokenEstimate: m.tokenEstimate ?? null, createdAt: m.createdAt }));
    }
    for (const u of usage) {
      lines.push(JSON.stringify({ type: "usage", turnSeq: u.turnSeq, inputTokens: u.inputTokens, outputTokens: u.outputTokens, model: u.model, createdAt: u.createdAt }));
    }

    return new Response(lines.join("\n") + "\n", {
      headers: {
        "Content-Type": "application/x-ndjson",
        "Content-Disposition": `attachment; filename="${id}.jsonl"`,
      },
    });
  });

  app.post("/api/export/encrypted", async (c) => {
    const body = await c.req.json<{ password?: unknown; nousId?: unknown; since?: unknown }>();
    const password = body.password;
    if (typeof password !== "string" || password.length < 8) {
      return c.json({ error: "password must be at least 8 characters" }, 400);
    }

    const nousId = typeof body.nousId === "string" ? body.nousId : undefined;
    const since = typeof body.since === "string" ? body.since : undefined;

    const sessions = store.listSessionsFiltered({
      ...(nousId ? { nousId } : {}),
      ...(since ? { since } : {}),
    });
    const lines: string[] = [];
    for (const s of sessions) {
      lines.push(JSON.stringify({ type: "session", ...s }));
      const messages = store.getHistory(s.id, { excludeDistilled: false });
      for (const m of messages) {
        lines.push(JSON.stringify({ type: "message", sessionId: s.id, seq: m.seq, role: m.role, content: m.content, createdAt: m.createdAt }));
      }
    }
    const plaintext = Buffer.from(lines.join("\n") + "\n", "utf8");

    const salt = randomBytes(32);
    const N = 16384, r = 8, p = 1;
    const key = scryptSync(password, salt, 32, { N, r, p });

    const iv = randomBytes(12);
    const cipher = createCipheriv("aes-256-gcm", key, iv);
    const ct = Buffer.concat([cipher.update(plaintext), cipher.final()]);
    const authTag = cipher.getAuthTag();
    const data = Buffer.concat([ct, authTag]).toString("base64");

    const envelope = {
      v: 1,
      kdf: "scrypt",
      salt: salt.toString("base64"),
      n: N, r, p,
      iv: iv.toString("base64"),
      cipher: "aes-256-gcm",
      data,
      sessionCount: sessions.length,
      exportedAt: new Date().toISOString(),
    };

    return new Response(JSON.stringify(envelope), {
      headers: {
        "Content-Type": "application/json",
        "Content-Disposition": `attachment; filename="aletheia-export-${Date.now()}.enc.json"`,
      },
    });
  });

  return app;
}
