// SSE event stream route
import { Hono } from "hono";
import { eventBus, type EventName } from "../../koina/event-bus.js";
import type { RouteDeps, RouteRefs } from "./deps.js";

export function eventRoutes(deps: RouteDeps, _refs: RouteRefs): Hono {
  const app = new Hono();
  const { manager } = deps;

  app.get("/api/events", (c) => {
    const encoder = new TextEncoder();
    let closed = false;

    const stream = new ReadableStream({
      start(controller) {
        const activeTurns = manager.getActiveTurnsByNous();
        controller.enqueue(encoder.encode(`event: init\ndata: ${JSON.stringify({ activeTurns })}\n\n`));

        const forward = (eventName: string) => (data: unknown) => {
          if (closed) return;
          try {
            controller.enqueue(encoder.encode(`event: ${eventName}\ndata: ${JSON.stringify(data)}\n\n`));
          } catch { closed = true; }
        };

        const handlers: Array<[EventName, (data: unknown) => void]> = [
          ["turn:before", forward("turn:before")],
          ["turn:after", forward("turn:after")],
          ["tool:called", forward("tool:called")],
          ["tool:failed", forward("tool:failed")],
          ["status:update", forward("status:update")],
          ["session:created", forward("session:created")],
          ["session:archived", forward("session:archived")],
          ["distill:before", forward("distill:before")],
          ["distill:stage", forward("distill:stage")],
          ["distill:after", forward("distill:after")],
        ];

        for (const [event, handler] of handlers) {
          eventBus.on(event, handler);
        }

        const pingInterval = setInterval(() => {
          if (closed) return;
          try { controller.enqueue(encoder.encode(`: ping\n\n`)); }
          catch { closed = true; }
        }, 15_000);

        c.req.raw.signal.addEventListener("abort", () => {
          closed = true;
          clearInterval(pingInterval);
          for (const [event, handler] of handlers) {
            eventBus.off(event, handler);
          }
          try { controller.close(); } catch { /* already closed */ }
        });
      },
    });

    return new Response(stream, {
      headers: {
        "Content-Type": "text/event-stream",
        "Cache-Control": "no-cache",
        "Connection": "keep-alive",
        "X-Accel-Buffering": "no",
      },
    });
  });

  return app;
}
