// Web UI — Svelte SPA served at /ui, SSE events at /api/events
import { existsSync, readFileSync } from "node:fs";
import { extname, join } from "node:path";
import { Hono } from "hono";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import { paths } from "../taxis/paths.js";

interface EventClient {
  controller: ReadableStreamDefaultController;
  lastPing: number;
}

const eventClients = new Set<EventClient>();

export function broadcastEvent(event: string, data: unknown): void {
  const payload = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
  const encoded = new TextEncoder().encode(payload);
  for (const client of eventClients) {
    try {
      client.controller.enqueue(encoded);
    } catch {
      eventClients.delete(client);
    }
  }
}

const MIME_TYPES: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".ttf": "font/ttf",
  ".map": "application/json",
};

interface ManagerLike {
  getActiveTurnsByNous(): Record<string, number>;
}

export function createUiRoutes(
  config: AletheiaConfig,
  manager: ManagerLike | null,
  store: SessionStore,
): Hono {
  const app = new Hono();

  // SSE event stream — real-time updates
  app.get("/api/events", (c) => {
    const stream = new ReadableStream({
      start(controller) {
        const client: EventClient = { controller, lastPing: Date.now() };
        eventClients.add(client);

        const ping = setInterval(() => {
          try {
            controller.enqueue(new TextEncoder().encode(": ping\n\n"));
            client.lastPing = Date.now();
          } catch {
            clearInterval(ping);
            eventClients.delete(client);
          }
        }, 30_000);

        // Send initial state
        const metrics = store.getMetrics();
        const initData = {
          agents: config.agents.list.map((a) => ({
            id: a.id,
            name: a.name ?? a.id,
          })),
          uptime: Math.round(process.uptime()),
          usage: metrics.usage,
          activeTurns: manager?.getActiveTurnsByNous() ?? {},
        };
        controller.enqueue(
          new TextEncoder().encode(`event: init\ndata: ${JSON.stringify(initData)}\n\n`),
        );

        c.req.raw.signal?.addEventListener("abort", () => {
          clearInterval(ping);
          eventClients.delete(client);
        });
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

  // Serve built Svelte UI from ui/dist/, fall back to inline dashboard
  const distDir = join(paths.root, "ui", "dist");
  const hasBuiltUi = existsSync(join(distDir, "index.html"));

  if (hasBuiltUi) {
    // Serve static assets with proper MIME types and caching
    app.get("/ui/*", (c) => {
      // Strip /ui prefix to get the file path within dist/
      let filePath = c.req.path.replace(/^\/ui\/?/, "");
      if (filePath === "" || filePath === "/") filePath = "index.html";

      const fullPath = join(distDir, filePath);

      // Prevent directory traversal
      if (!fullPath.startsWith(distDir)) {
        return c.text("Forbidden", 403);
      }

      if (existsSync(fullPath)) {
        const ext = extname(fullPath);
        const mime = MIME_TYPES[ext] ?? "application/octet-stream";
        const content = readFileSync(fullPath);

        // Hashed assets get long cache, index.html gets no-cache for SPA updates
        const isHashedAsset = filePath.startsWith("assets/");
        const cacheControl = isHashedAsset
          ? "public, max-age=31536000, immutable"
          : "no-cache";

        return new Response(content, {
          headers: {
            "Content-Type": mime,
            "Cache-Control": cacheControl,
          },
        });
      }

      // SPA fallback — serve index.html for any unmatched /ui/* route
      const indexContent = readFileSync(join(distDir, "index.html"));
      return new Response(indexContent, {
        headers: {
          "Content-Type": "text/html; charset=utf-8",
          "Cache-Control": "no-cache",
        },
      });
    });
  } else {
    // Fallback: inline Preact dashboard when ui/dist/ not built
    app.get("/ui", (c) => c.html(fallbackDashboardHtml(config)));
    app.get("/ui/*", (c) => c.html(fallbackDashboardHtml(config)));
  }

  return app;
}

function fallbackDashboardHtml(config: AletheiaConfig): string {
  const agents = config.agents.list.map((a) => ({
    id: a.id,
    name: a.name ?? a.id,
  }));

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Aletheia</title>
  <style>
    :root {
      --bg: #0d1117; --surface: #161b22; --border: #30363d;
      --text: #e6edf3; --text-dim: #8b949e; --accent: #58a6ff;
      --green: #3fb950; --font: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
      --mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { font-family: var(--font); background: var(--bg); color: var(--text); font-size: 14px; display: flex; align-items: center; justify-content: center; min-height: 100vh; }
    .container { text-align: center; max-width: 500px; padding: 32px; }
    h1 { font-size: 24px; margin-bottom: 8px; }
    p { color: var(--text-dim); margin-bottom: 24px; }
    .agents { display: flex; flex-wrap: wrap; gap: 8px; justify-content: center; }
    .agent { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 8px 16px; font-size: 13px; }
    .note { margin-top: 32px; font-size: 12px; color: var(--text-dim); font-family: var(--mono); }
  </style>
</head>
<body>
  <div class="container">
    <h1>Aletheia</h1>
    <p>Web UI not built. Run <code>cd ui && npm install && npm run build</code> to enable the full interface.</p>
    <div class="agents">
      ${agents.map((a) => `<div class="agent">${a.name}</div>`).join("\n      ")}
    </div>
    <p class="note">API endpoints available at /api/*</p>
  </div>
</body>
</html>`;
}
