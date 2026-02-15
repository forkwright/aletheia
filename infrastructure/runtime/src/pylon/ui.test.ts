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

  it("/ui returns HTML dashboard", async () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    const res = await app.request("/ui");
    expect(res.status).toBe(200);
    const html = await res.text();
    expect(html).toContain("<!DOCTYPE html>");
    expect(html).toContain("Aletheia");
    expect(html).toContain("Syn");
    expect(html).toContain("Eiron");
  });

  it("/ui/* returns same dashboard HTML", async () => {
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

  it("fallback dashboard contains agent names", async () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    const res = await app.request("/ui");
    const html = await res.text();
    expect(html).toContain("Syn");
    expect(html).toContain("Eiron");
  });

  it("fallback dashboard includes CSS styles", async () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    const res = await app.request("/ui");
    const html = await res.text();
    expect(html).toContain("<style>");
    expect(html).toContain("--bg:");
  });

  it("fallback dashboard shows build instructions", async () => {
    const app = createUiRoutes(makeConfig(), null, makeStore());
    const res = await app.request("/ui");
    const html = await res.text();
    expect(html).toContain("npm run build");
  });
});

describe("broadcastEvent", () => {
  it("is exported and callable", () => {
    expect(typeof broadcastEvent).toBe("function");
    // Should not throw even when no clients connected
    broadcastEvent("test", { hello: "world" });
  });
});
