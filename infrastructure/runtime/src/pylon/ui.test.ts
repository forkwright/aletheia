// Web UI route tests
import { describe, expect, it, vi } from "vitest";
import { broadcastEvent, createUiRoutes } from "./ui.js";

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

  // SSE events are now served by routes/events.ts, not ui.ts
});

describe("broadcastEvent", () => {
  it("is exported as no-op for backward compatibility", () => {
    expect(typeof broadcastEvent).toBe("function");
    // Should not throw — it's a no-op now
    broadcastEvent("test", { hello: "world" });
  });
});
