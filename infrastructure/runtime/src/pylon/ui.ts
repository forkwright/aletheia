// Web UI — embedded SPA served at /ui, SSE events at /api/events
import { Hono } from "hono";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";

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

export function createUiRoutes(
  config: AletheiaConfig,
  _manager: unknown,
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

  // Serve embedded SPA — single HTML file with Preact + HTM
  app.get("/ui", (c) => c.html(dashboardHtml(config)));
  app.get("/ui/*", (c) => c.html(dashboardHtml(config)));

  return app;
}

function dashboardHtml(config: AletheiaConfig): string {
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
      --green: #3fb950; --red: #f85149; --yellow: #d29922;
      --font: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica, Arial, sans-serif;
      --mono: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace;
    }
    * { box-sizing: border-box; margin: 0; padding: 0; }
    body { font-family: var(--font); background: var(--bg); color: var(--text); font-size: 14px; }
    a { color: var(--accent); text-decoration: none; }
    .container { max-width: 1200px; margin: 0 auto; padding: 16px; }
    header { display: flex; align-items: center; gap: 16px; padding: 12px 0; border-bottom: 1px solid var(--border); margin-bottom: 16px; }
    header h1 { font-size: 18px; font-weight: 600; }
    header .uptime { color: var(--text-dim); font-size: 13px; }
    .grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 12px; }
    .card { background: var(--surface); border: 1px solid var(--border); border-radius: 8px; padding: 16px; }
    .card h2 { font-size: 14px; font-weight: 600; margin-bottom: 8px; color: var(--text-dim); text-transform: uppercase; letter-spacing: 0.5px; }
    .card .value { font-size: 28px; font-weight: 700; }
    .card .sub { font-size: 12px; color: var(--text-dim); margin-top: 4px; }
    .agent-list { margin-top: 16px; }
    .agent-row { display: flex; align-items: center; gap: 12px; padding: 10px 16px; background: var(--surface); border: 1px solid var(--border); border-radius: 8px; margin-bottom: 8px; cursor: pointer; transition: border-color 0.15s; }
    .agent-row:hover { border-color: var(--accent); }
    .agent-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; }
    .agent-dot.active { background: var(--green); }
    .agent-dot.idle { background: var(--text-dim); }
    .agent-name { font-weight: 600; flex: 1; }
    .agent-stats { display: flex; gap: 16px; font-size: 12px; color: var(--text-dim); }
    .sessions-panel { margin-top: 16px; }
    .session-item { padding: 8px 16px; background: var(--surface); border: 1px solid var(--border); border-radius: 6px; margin-bottom: 6px; font-size: 13px; display: flex; justify-content: space-between; align-items: center; }
    .session-item .key { font-family: var(--mono); color: var(--accent); }
    .session-item .meta { color: var(--text-dim); font-size: 12px; }
    .msg { padding: 8px 12px; margin: 4px 0; border-radius: 6px; font-size: 13px; }
    .msg.user { background: #1a3354; border-left: 3px solid var(--accent); }
    .msg.assistant { background: #1a3320; border-left: 3px solid var(--green); }
    .msg.tool { background: #33231a; border-left: 3px solid var(--yellow); font-family: var(--mono); font-size: 12px; }
    .nav { display: flex; gap: 8px; margin-bottom: 16px; }
    .nav button { background: var(--surface); border: 1px solid var(--border); color: var(--text); padding: 6px 14px; border-radius: 6px; cursor: pointer; font-size: 13px; }
    .nav button.active { border-color: var(--accent); color: var(--accent); }
    .badge { display: inline-block; padding: 2px 8px; border-radius: 12px; font-size: 11px; font-weight: 600; }
    .badge.ok { background: rgba(63,185,80,0.15); color: var(--green); }
    .badge.down { background: rgba(248,81,73,0.15); color: var(--red); }
    table { width: 100%; border-collapse: collapse; font-size: 13px; }
    th { text-align: left; padding: 8px; color: var(--text-dim); border-bottom: 1px solid var(--border); font-weight: 600; }
    td { padding: 8px; border-bottom: 1px solid var(--border); }
    .empty { color: var(--text-dim); text-align: center; padding: 32px; }
    #app { min-height: 100vh; }
  </style>
</head>
<body>
  <div id="app"></div>
  <script type="module">
    import { h, render } from "https://esm.sh/preact@10.25.4";
    import { useState, useEffect, useRef } from "https://esm.sh/preact@10.25.4/hooks";
    import htm from "https://esm.sh/htm@3.1.1";
    const html = htm.bind(h);

    const AGENTS = ${JSON.stringify(agents)};
    const BASE = location.origin;

    function fetchApi(path) {
      return fetch(BASE + path).then(r => r.json());
    }

    function fmtTokens(n) {
      if (!n) return "0";
      if (n > 1000000) return (n / 1000000).toFixed(1) + "M";
      if (n > 1000) return (n / 1000).toFixed(0) + "k";
      return String(n);
    }
    function fmtTime(seconds) {
      const h = Math.floor(seconds / 3600);
      const m = Math.floor((seconds % 3600) / 60);
      return h > 0 ? h + "h " + m + "m" : m + "m";
    }
    function timeSince(dateStr) {
      if (!dateStr) return "never";
      const ms = Date.now() - new Date(dateStr).getTime();
      const mins = Math.floor(ms / 60000);
      if (mins < 1) return "just now";
      if (mins < 60) return mins + "m ago";
      const hours = Math.floor(mins / 60);
      if (hours < 24) return hours + "h ago";
      return Math.floor(hours / 24) + "d ago";
    }

    function App() {
      const [view, setView] = useState("dashboard");
      const [metrics, setMetrics] = useState(null);
      const [selectedAgent, setSelectedAgent] = useState(null);
      const [sessions, setSessions] = useState([]);
      const [selectedSession, setSelectedSession] = useState(null);
      const [history, setHistory] = useState([]);
      const [services, setServices] = useState([]);
      const [costs, setCosts] = useState(null);

      useEffect(() => {
        fetchApi("/api/metrics").then(setMetrics);
        const iv = setInterval(() => fetchApi("/api/metrics").then(setMetrics), 15000);
        return () => clearInterval(iv);
      }, []);

      useEffect(() => {
        if (metrics?.services) setServices(metrics.services);
      }, [metrics]);

      const openAgent = (id) => {
        setSelectedAgent(id);
        setView("agent");
        fetchApi("/api/sessions?nousId=" + id).then(d => setSessions(d.sessions || []));
      };

      const openSession = (id) => {
        setSelectedSession(id);
        setView("session");
        fetchApi("/api/sessions/" + id + "/history?limit=50").then(d => setHistory(d.messages || []));
      };

      const openCosts = () => {
        setView("costs");
        fetchApi("/api/costs/summary").then(setCosts);
      };

      if (!metrics) return html\`<div class="container"><div class="empty">Loading...</div></div>\`;

      return html\`
        <div class="container">
          <header>
            <h1>Aletheia</h1>
            <span class="uptime">Uptime: \${fmtTime(metrics.uptime)}</span>
          </header>

          <div class="nav">
            <button class=\${view === "dashboard" ? "active" : ""} onClick=\${() => setView("dashboard")}>Dashboard</button>
            <button class=\${view === "costs" ? "active" : ""} onClick=\${openCosts}>Costs</button>
          </div>

          \${view === "dashboard" && html\`<\${Dashboard} metrics=\${metrics} services=\${services} onAgent=\${openAgent} />\`}
          \${view === "agent" && html\`<\${AgentView} id=\${selectedAgent} sessions=\${sessions} onSession=\${openSession} onBack=\${() => setView("dashboard")} />\`}
          \${view === "session" && html\`<\${SessionView} id=\${selectedSession} history=\${history} onBack=\${() => openAgent(selectedAgent)} />\`}
          \${view === "costs" && html\`<\${CostsView} costs=\${costs} onBack=\${() => setView("dashboard")} />\`}
        </div>
      \`;
    }

    function Dashboard({ metrics, services, onAgent }) {
      const nous = metrics.nous || [];
      const usage = metrics.usage || {};

      return html\`
        <div class="grid">
          <div class="card">
            <h2>Tokens</h2>
            <div class="value">\${fmtTokens(usage.totalInputTokens + usage.totalOutputTokens)}</div>
            <div class="sub">\${fmtTokens(usage.totalInputTokens)} in / \${fmtTokens(usage.totalOutputTokens)} out</div>
          </div>
          <div class="card">
            <h2>Cache Hit Rate</h2>
            <div class="value">\${usage.cacheHitRate || 0}%</div>
            <div class="sub">\${fmtTokens(usage.totalCacheReadTokens)} cached tokens</div>
          </div>
          <div class="card">
            <h2>Turns</h2>
            <div class="value">\${usage.turnCount || 0}</div>
            <div class="sub">Across all agents</div>
          </div>
          <div class="card">
            <h2>Services</h2>
            <div class="value">\${services.length > 0 ? services.filter(s => s.healthy).length + "/" + services.length : "—"}</div>
            <div class="sub">\${services.map(s => html\`<span class=\${"badge " + (s.healthy ? "ok" : "down")}>\${s.name}</span> \`)}</div>
          </div>
        </div>

        <div class="agent-list">
          <h2 style="color: var(--text-dim); font-size: 13px; text-transform: uppercase; letter-spacing: 0.5px; margin-bottom: 8px;">Nous</h2>
          \${nous.map(a => html\`
            <div class="agent-row" onClick=\${() => onAgent(a.id)}>
              <div class=\${"agent-dot " + (a.activeSessions > 0 ? "active" : "idle")}></div>
              <span class="agent-name">\${a.name}</span>
              <div class="agent-stats">
                <span>\${a.activeSessions} sessions</span>
                <span>\${a.totalMessages} msgs</span>
                <span>\${timeSince(a.lastActivity)}</span>
                <span>\${a.tokens ? fmtTokens(a.tokens.input) + " in" : "—"}</span>
              </div>
            </div>
          \`)}
        </div>
      \`;
    }

    function AgentView({ id, sessions, onSession, onBack }) {
      const agent = AGENTS.find(a => a.id === id);
      return html\`
        <div>
          <button onClick=\${onBack} style="margin-bottom: 12px; background: none; border: none; color: var(--accent); cursor: pointer; font-size: 13px;">← Back</button>
          <h2 style="font-size: 18px; margin-bottom: 12px;">\${agent?.name || id}</h2>
          <div class="sessions-panel">
            \${sessions.length === 0 ? html\`<div class="empty">No sessions</div>\` :
              sessions.map(s => html\`
                <div class="session-item" onClick=\${() => onSession(s.id)}>
                  <span class="key">\${s.sessionKey || s.id.slice(0, 12)}</span>
                  <span class="meta">\${s.messageCount || "?"} msgs · \${timeSince(s.lastActivity || s.updatedAt)}</span>
                </div>
              \`)
            }
          </div>
        </div>
      \`;
    }

    function SessionView({ id, history, onBack }) {
      return html\`
        <div>
          <button onClick=\${onBack} style="margin-bottom: 12px; background: none; border: none; color: var(--accent); cursor: pointer; font-size: 13px;">← Back</button>
          <h2 style="font-size: 16px; margin-bottom: 12px; font-family: var(--mono);">\${id}</h2>
          <div>
            \${history.length === 0 ? html\`<div class="empty">No messages</div>\` :
              history.map(m => {
                const role = m.role || "user";
                const cls = role === "user" ? "user" : role === "tool" ? "tool" : "assistant";
                const text = typeof m.content === "string" ? m.content : JSON.stringify(m.content).slice(0, 300);
                return html\`<div class=\${"msg " + cls}><strong>\${role}</strong>: \${text.slice(0, 500)}</div>\`;
              })
            }
          </div>
        </div>
      \`;
    }

    function CostsView({ costs, onBack }) {
      if (!costs) return html\`<div class="empty">Loading...</div>\`;
      return html\`
        <div>
          <button onClick=\${onBack} style="margin-bottom: 12px; background: none; border: none; color: var(--accent); cursor: pointer; font-size: 13px;">← Back</button>
          <h2 style="font-size: 18px; margin-bottom: 12px;">Cost Attribution</h2>
          <div class="card" style="margin-bottom: 16px;">
            <h2>Total</h2>
            <div class="value">$\${costs.totalCost?.toFixed(4) || "0.0000"}</div>
          </div>
          <table>
            <thead><tr><th>Agent</th><th>Cost</th><th>Turns</th></tr></thead>
            <tbody>
              \${(costs.agents || []).map(a => html\`
                <tr>
                  <td>\${a.agentId}</td>
                  <td>$\${(a.totalCost || a.cost || 0).toFixed(4)}</td>
                  <td>\${a.turns || 0}</td>
                </tr>
              \`)}
            </tbody>
          </table>
        </div>
      \`;
    }

    render(html\`<\${App} />\`, document.getElementById("app"));
  </script>
</body>
</html>`;
}
