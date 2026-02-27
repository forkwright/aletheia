// Tests for AgoraRegistry — channel provider lifecycle and routing
import { describe, it, expect, vi, beforeEach } from "vitest";
import { AgoraRegistry } from "./registry.js";
import type { ChannelContext, ChannelProvider, ChannelSendResult } from "./types.js";

function mockProvider(id: string, overrides?: Partial<ChannelProvider>): ChannelProvider {
  return {
    id,
    name: id.charAt(0).toUpperCase() + id.slice(1),
    capabilities: {
      threads: false,
      reactions: false,
      typing: false,
      media: false,
      streaming: false,
      richFormatting: false,
      maxTextLength: 2000,
    },
    start: vi.fn().mockResolvedValue(undefined),
    send: vi.fn().mockResolvedValue({ sent: true } satisfies ChannelSendResult),
    stop: vi.fn().mockResolvedValue(undefined),
    ...overrides,
  };
}

function mockContext(): ChannelContext {
  return {
    dispatch: vi.fn().mockResolvedValue({ text: "ok" }),
    config: {} as ChannelContext["config"],
    store: {} as ChannelContext["store"],
    manager: {} as ChannelContext["manager"],
    abortSignal: new AbortController().signal,
  };
}

describe("AgoraRegistry", () => {
  let registry: AgoraRegistry;

  beforeEach(() => {
    registry = new AgoraRegistry();
  });

  describe("register", () => {
    it("registers a provider", () => {
      const p = mockProvider("signal");
      registry.register(p);
      expect(registry.has("signal")).toBe(true);
      expect(registry.get("signal")).toBe(p);
      expect(registry.size).toBe(1);
    });

    it("throws on duplicate registration", () => {
      registry.register(mockProvider("signal"));
      expect(() => registry.register(mockProvider("signal"))).toThrow(
        'Channel provider "signal" already registered',
      );
    });

    it("lists all registered provider IDs", () => {
      registry.register(mockProvider("signal"));
      registry.register(mockProvider("slack"));
      expect(registry.list()).toEqual(["signal", "slack"]);
    });
  });

  describe("startAll", () => {
    it("calls start on all providers", async () => {
      const p1 = mockProvider("signal");
      const p2 = mockProvider("slack");
      registry.register(p1);
      registry.register(p2);

      const ctx = mockContext();
      await registry.startAll(ctx);

      expect(p1.start).toHaveBeenCalledWith(ctx);
      expect(p2.start).toHaveBeenCalledWith(ctx);
    });

    it("does not throw if one provider fails to start", async () => {
      const good = mockProvider("signal");
      const bad = mockProvider("slack", {
        start: vi.fn().mockRejectedValue(new Error("boom")),
      });
      registry.register(good);
      registry.register(bad);

      // Should not throw — bad channel logs error but doesn't block
      await registry.startAll(mockContext());
      expect(good.start).toHaveBeenCalled();
    });

    it("ignores second call", async () => {
      const p = mockProvider("signal");
      registry.register(p);
      const ctx = mockContext();

      await registry.startAll(ctx);
      await registry.startAll(ctx); // Second call — should be no-op

      expect(p.start).toHaveBeenCalledTimes(1);
    });
  });

  describe("stopAll", () => {
    it("calls stop on all providers", async () => {
      const p1 = mockProvider("signal");
      const p2 = mockProvider("slack");
      registry.register(p1);
      registry.register(p2);

      await registry.startAll(mockContext());
      await registry.stopAll();

      expect(p1.stop).toHaveBeenCalled();
      expect(p2.stop).toHaveBeenCalled();
    });
  });

  describe("send", () => {
    it("routes to the correct provider", async () => {
      const signal = mockProvider("signal");
      const slack = mockProvider("slack");
      registry.register(signal);
      registry.register(slack);

      const result = await registry.send("signal", { to: "+1234", text: "hello" });
      expect(result.sent).toBe(true);
      expect(signal.send).toHaveBeenCalledWith({ to: "+1234", text: "hello" });
      expect(slack.send).not.toHaveBeenCalled();
    });

    it("returns error for unknown channel", async () => {
      const result = await registry.send("discord", { to: "foo", text: "bar" });
      expect(result.sent).toBe(false);
      expect(result.error).toContain("discord");
    });
  });

  describe("probeAll", () => {
    it("probes all providers with probe()", async () => {
      const signal = mockProvider("signal", {
        probe: vi.fn().mockResolvedValue({ ok: true, latencyMs: 10 }),
      });
      const slack = mockProvider("slack", {
        probe: vi.fn().mockResolvedValue({ ok: false, error: "disconnected" }),
      });
      registry.register(signal);
      registry.register(slack);

      const results = await registry.probeAll();
      expect(results.get("signal")).toEqual({ ok: true, latencyMs: 10 });
      expect(results.get("slack")).toEqual({ ok: false, error: "disconnected" });
    });

    it("assumes OK for providers without probe()", async () => {
      const p = mockProvider("signal"); // no probe method
      registry.register(p);

      const results = await registry.probeAll();
      expect(results.get("signal")).toEqual({ ok: true });
    });

    it("handles probe errors gracefully", async () => {
      const p = mockProvider("signal", {
        probe: vi.fn().mockRejectedValue(new Error("timeout")),
      });
      registry.register(p);

      const results = await registry.probeAll();
      expect(results.get("signal")?.ok).toBe(false);
      expect(results.get("signal")?.error).toContain("timeout");
    });
  });

  describe("getFirst", () => {
    it("returns first provider without predicate", () => {
      registry.register(mockProvider("signal"));
      registry.register(mockProvider("slack"));
      expect(registry.getFirst()?.id).toBe("signal");
    });

    it("returns first matching provider with predicate", () => {
      registry.register(mockProvider("signal"));
      registry.register(mockProvider("slack", {
        capabilities: {
          threads: true,
          reactions: true,
          typing: false,
          media: true,
          streaming: true,
          richFormatting: true,
          maxTextLength: 4000,
        },
      }));
      const result = registry.getFirst((p) => p.capabilities.threads);
      expect(result?.id).toBe("slack");
    });

    it("returns undefined when nothing matches", () => {
      registry.register(mockProvider("signal"));
      expect(registry.getFirst((p) => p.id === "discord")).toBeUndefined();
    });
  });
});
