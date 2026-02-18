// Web UI route tests
import { describe, it, expect, vi } from "vitest";
import { createUiRoutes, broadcastEvent } from "./ui.js";

function makeConfig() {
  return {
    agents: {
      list: [
        { id: "syn", name: "Syn" },
        { id: "eiron", name: "Eiron" },
      ],
    },
  } as never;
}

function makeStore() {
  return {
    getMetrics: vi.fn().mockReturnValue({
      usage: { totalInputTokens: 5000, totalOutputTokens: 2000, totalCacheReadTokens: 100, totalCacheWriteTokens: 50, turnCount: 10 },
    }),
  } as never;
}

describe("createUiRoutes", () => {
  it("creates a Hono app with routes", () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    expect(app).toBeDefined();
    expect(app.fetch).toBeDefined();
  });

  it("/ui returns HTML", async () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    const res = await app.request("/ui");
    expect(res.status).toBe(200);
    const html = await res.text();
    expect(html).toContain("<!DOCTYPE html>");
    expect(html).toContain("<title>");
  });

  it("/ui/* serves SPA (fallback to index.html)", async () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    const res = await app.request("/ui/agents/syn");
    expect(res.status).toBe(200);
    const html = await res.text();
    expect(html).toContain("<!DOCTYPE html>");
  });

  it("/api/events returns SSE stream", async () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    const res = await app.request("/api/events");
    expect(res.status).toBe(200);
    expect(res.headers.get("Content-Type")).toBe("text/event-stream");
    expect(res.headers.get("Cache-Control")).toBe("no-cache");
  });
});

describe("broadcastEvent", () => {
  it("is exported and callable", () => {
    expect(typeof broadcastEvent).toBe("function");
    broadcastEvent("test", { hello: "world" });
  });
});
