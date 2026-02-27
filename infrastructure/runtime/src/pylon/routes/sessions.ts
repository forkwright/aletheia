// Session routes — list, history, send, stream, archive, distill, queue, checkpoints, fork
import { Hono } from "hono";
import { createLogger, withTurnAsync } from "../../koina/logger.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

const log = createLogger("pylon");

export function sessionRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { manager, store } = deps;

  app.get("/api/sessions", (c) => {
    const nousId = c.req.query("nousId");
    const sessions = store.listSessions(nousId);
    const canonical: Record<string, string> = {};
    if (nousId) {
      const key = store.getCanonicalSessionKey(nousId);
      if (key) canonical[nousId] = key;
    } else {
      const seen = new Set<string>();
      for (const s of sessions) {
        if (!seen.has(s.nousId)) {
          seen.add(s.nousId);
          const key = store.getCanonicalSessionKey(s.nousId);
          if (key) canonical[s.nousId] = key;
        }
      }
    }
    return c.json({ sessions, canonical });
  });

  app.get("/api/sessions/:id/history", (c) => {
    const id = c.req.param("id");
    const limit = parseInt(c.req.query("limit") ?? "100", 10);
    const includeDistilled = c.req.query("includeDistilled") === "true";
    const history = store.getHistory(id, { limit, excludeDistilled: !includeDistilled });
    return c.json({ messages: history });
  });

  app.post("/api/sessions/send", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "Invalid JSON body" }, 400);
    }

    const { agentId, message, sessionKey, media } = body as {
      agentId: string;
      message: string;
      sessionKey?: string;
      media?: Array<{ contentType: string; data: string; filename?: string }>;
    };

    if (!agentId || !message) {
      return c.json({ error: "agentId and message required" }, 400);
    }

    let resolvedKey = sessionKey ?? "main";
    if (resolvedKey.startsWith("signal:")) {
      const signalPeerId = resolvedKey.slice("signal:".length);
      const routedOwner = store.resolveRoute("signal", "group", signalPeerId)
        ?? store.resolveRoute("signal", "dm", signalPeerId);
      if (routedOwner && routedOwner !== agentId) {
        resolvedKey = `web:${Date.now()}`;
        log.warn(`API session key ownership mismatch: signal key belongs to ${routedOwner}, not ${agentId}`);
      }
    } else if (!resolvedKey.startsWith("agent:") && !resolvedKey.startsWith("cron:") && !resolvedKey.startsWith("spawn:")) {
      const canonical = store.getCanonicalSessionKey(agentId);
      if (canonical) {
        if (canonical !== resolvedKey) {
          log.info(`API session converged: "${resolvedKey}" → "${canonical}" for ${agentId}`);
        }
        resolvedKey = canonical;
      }
    }

    try {
      const result = await withTurnAsync(
        { channel: "api", nousId: agentId, sessionKey: resolvedKey },
        () => manager.handleMessage({
          text: message,
          nousId: agentId,
          sessionKey: resolvedKey,
          ...(media?.length ? { media } : {}),
        }),
      );
      return c.json({
        response: result.text,
        sessionId: result.sessionId,
        toolCalls: result.toolCalls,
        ...(result.error ? { error: result.error } : {}),
        usage: {
          inputTokens: result.inputTokens,
          outputTokens: result.outputTokens,
          cacheReadTokens: result.cacheReadTokens,
          cacheWriteTokens: result.cacheWriteTokens,
        },
      });
    } catch (error) {
      const msg =
        error instanceof Error ? error.message : String(error);
      log.error(`Session send failed: ${msg}`);
      return c.json({ error: "Internal error processing message" }, 500);
    }
  });

  app.post("/api/sessions/stream", async (c) => {
    let body: Record<string, unknown>;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "Invalid JSON body" }, 400);
    }

    const { agentId, message, sessionKey, media } = body as {
      agentId: string;
      message: string;
      sessionKey?: string;
      media?: Array<{ contentType: string; data: string; filename?: string }>;
    };

    if (!agentId || !message) {
      return c.json({ error: "agentId and message required" }, 400);
    }

    if (typeof manager.handleMessageStreaming !== "function") {
      return c.json({ error: "Streaming not implemented" }, 501);
    }

    const validMedia: Array<{ contentType: string; data: string; filename?: string }> = [];
    if (media?.length) {
      log.info(`Stream request has ${media.length} media attachment(s)`);
      const maxBytes = 25 * 1024 * 1024;
      for (const item of media) {
        if (!item.contentType || !item.data) continue;
        const estimatedSize = Math.ceil(item.data.length * 0.75);
        if (estimatedSize > maxBytes) {
          log.warn(`Skipping oversized webchat attachment (${Math.round(estimatedSize / 1024)}KB)`);
          continue;
        }
        if (!/^(image|audio|application|text)\//i.test(item.contentType)) {
          log.warn(`Skipping unsupported media type: ${item.contentType}`);
          continue;
        }
        validMedia.push(item);
      }
    }

    if (validMedia.length > 0) {
      log.info(`Passing ${validMedia.length} valid media to manager (types: ${validMedia.map(m => m.contentType).join(", ")})`);
    }

    const encoder = new TextEncoder();
    const rawSessionKey = sessionKey ?? "main";

    let resolvedSessionKey = rawSessionKey;

    if (rawSessionKey.startsWith("signal:")) {
      const signalPeerId = rawSessionKey.slice("signal:".length);
      const routedOwner = store.resolveRoute("signal", "group", signalPeerId)
        ?? store.resolveRoute("signal", "dm", signalPeerId);
      if (routedOwner && routedOwner !== agentId) {
        resolvedSessionKey = `web:${agentId}`;
        log.warn(
          `Session key ownership mismatch: "${rawSessionKey}" belongs to ${routedOwner}, ` +
          `not ${agentId}. Reassigned to "${resolvedSessionKey}".`,
        );
      }
    } else if (!rawSessionKey.startsWith("agent:") && !rawSessionKey.startsWith("cron:") && !rawSessionKey.startsWith("spawn:")) {
      const canonical = store.getCanonicalSessionKey(agentId);
      if (canonical) {
        resolvedSessionKey = canonical;
        if (canonical !== rawSessionKey) {
          log.info(
            `Webchat session converged: "${rawSessionKey}" → "${canonical}" for ${agentId}`,
          );
        }
      }
    }

    let webchatThreadId: string | undefined;
    let webchatBindingId: string | undefined;
    let webchatLockKey: string | undefined;
    try {
      const identity = "anonymous";
      const channelKey = `web:${identity}:${agentId}`;
      const thread = manager.sessionStore.resolveThread(agentId, identity);
      const binding = manager.sessionStore.resolveBinding(thread.id, "webchat", channelKey);
      webchatThreadId = thread.id;
      webchatBindingId = binding.id;
      webchatLockKey = `binding:${binding.id}`;
    } catch (error) {
      log.warn(`Webchat thread resolution failed: ${error instanceof Error ? error.message : error}`);
    }

    const newTopicMatch = message.trim().match(/^\/new(?:\s+(.+))?$/i);
    if (newTopicMatch) {
      const topicLabel = newTopicMatch[1]?.trim() ?? "";
      const boundaryContent = topicLabel ? `[TOPIC: ${topicLabel}]` : "[TOPIC]";
      const ackText = topicLabel ? `New topic: ${topicLabel}` : "New topic started.";
      try {
        const session = store.findOrCreateSession(agentId, resolvedSessionKey);
        store.appendMessage(session.id, "user", boundaryContent, { tokenEstimate: 10 });
      } catch (error) {
        log.warn(`Topic boundary insert failed: ${error instanceof Error ? error.message : error}`);
      }
      const enc = new TextEncoder();
      const topicStream = new ReadableStream({
        start(ctrl) {
          const turnId = `webchat:topic:${Date.now()}`;
          ctrl.enqueue(enc.encode(`event: turn_start\ndata: ${JSON.stringify({ type: "turn_start", sessionId: "", nousId: agentId, turnId })}\n\n`));
          ctrl.enqueue(enc.encode(`event: text_delta\ndata: ${JSON.stringify({ type: "text_delta", text: ackText })}\n\n`));
          ctrl.enqueue(enc.encode(`event: turn_complete\ndata: ${JSON.stringify({ type: "turn_complete", outcome: { text: ackText, nousId: agentId, sessionId: "", model: "none", toolCalls: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 } })}\n\n`));
          ctrl.close();
        },
      });
      return new Response(topicStream, { headers: { "Content-Type": "text/event-stream", "Cache-Control": "no-cache", "X-Accel-Buffering": "no" } });
    }

    const stream = new ReadableStream({
      async start(controller) {
        const heartbeat = setInterval(() => {
          try {
            controller.enqueue(encoder.encode(":heartbeat\n\n"));
          } catch { /* stream already closed */ }
        }, 30_000);

        await withTurnAsync(
          { channel: "webchat", nousId: agentId, sessionKey: resolvedSessionKey },
          async () => {
            try {
              for await (const event of manager.handleMessageStreaming({
                text: message,
                nousId: agentId,
                sessionKey: resolvedSessionKey,
                ...(webchatThreadId ? { threadId: webchatThreadId } : {}),
                ...(webchatBindingId ? { bindingId: webchatBindingId } : {}),
                ...(webchatLockKey ? { lockKey: webchatLockKey } : {}),
                ...(validMedia.length > 0 ? { media: validMedia } : {}),
              })) {
                try {
                  const payload = `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`;
                  controller.enqueue(encoder.encode(payload));
                } catch {
                  break;
                }
              }
            } catch (error) {
              const msg = error instanceof Error ? error.message : String(error);
              log.error(`Stream error: ${msg}`);
              try {
                const payload = `event: error\ndata: ${JSON.stringify({ type: "error", message: "Internal error" })}\n\n`;
                controller.enqueue(encoder.encode(payload));
              } catch { /* client already disconnected */ }
            } finally {
              clearInterval(heartbeat);
              controller.close();
            }
          },
        );
      },
    });

    return new Response(stream, {
      headers: {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        "Connection": "keep-alive",
      },
    });
  });

  app.post("/api/sessions/:id/archive", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);
    store.archiveSession(id);
    return c.json({ ok: true, archived: id });
  });

  app.post("/api/sessions/:id/distill", async (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    try {
      await manager.triggerDistillation(id);
      return c.json({ ok: true, sessionId: id });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.error(`Distillation trigger failed: ${msg}`);
      return c.json({ error: "Failed to trigger distillation" }, 500);
    }
  });

  app.post("/api/sessions/:id/distill/cancel", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    try {
      const cancelled = manager.cancelDistillation(id);
      return c.json({ ok: true, cancelled });
    } catch (error) {
      log.error(`Failed to cancel distillation for ${id}`, { error });
      return c.json({ error: "Failed to cancel distillation" }, 500);
    }
  });

  app.get("/api/sessions/:id/checkpoints", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);
    return c.json({ sessionId: id, checkpoints: store.getCheckpoints(id) });
  });

  app.post("/api/sessions/:id/fork", async (c) => {
    const id = c.req.param("id");
    let body: Record<string, unknown>;
    try {
      body = await c.req.json();
    } catch {
      return c.json({ error: "Invalid JSON" }, 400);
    }

    const at = body["at"];
    if (typeof at !== "number" || at < 1) {
      return c.json({ error: "'at' (distillation number) required, must be >= 1" }, 400);
    }

    try {
      const result = store.forkSession(id, at);
      return c.json({
        ok: true,
        newSessionId: result.newSessionId,
        messagesCopied: result.messagesCopied,
        sourceSessionId: id,
        checkpoint: at,
      });
    } catch (error) {
      const msg = error instanceof Error ? error.message : String(error);
      log.warn(`Session fork failed: ${msg}`);
      return c.json({ error: msg }, 400);
    }
  });

  app.post("/api/sessions/:id/queue", async (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    const body = await c.req.json<{ text?: string; sender?: string }>().catch(() => null);
    if (!body?.text?.trim()) return c.json({ error: "text is required" }, 400);

    store.queueMessage(id, body.text.trim(), body.sender);
    return c.json({ ok: true, queued: true, queueLength: store.getQueueLength(id) });
  });

  app.get("/api/sessions/:id/queue", (c) => {
    const id = c.req.param("id");
    const session = store.findSessionById(id);
    if (!session) return c.json({ error: "Session not found" }, 404);

    return c.json({ sessionId: id, queueLength: store.getQueueLength(id) });
  });

  return app;
}
