// Signal daemon tests — config extraction, lifecycle
import { describe, expect, it, vi } from "vitest";
import { daemonOptsFromConfig } from "./daemon.js";

describe("daemonOptsFromConfig", () => {
  it("creates opts from minimal account config", () => {
    const opts = daemonOptsFromConfig("main", {
      account: "+1234567890",
    } as never);
    expect(opts.account).toBe("+1234567890");
    expect(opts.httpHost).toBeUndefined();
    expect(opts.httpPort).toBeUndefined();
  });

  it("uses accountId when account field missing", () => {
    const opts = daemonOptsFromConfig("+9876543210", {} as never);
    expect(opts.account).toBe("+9876543210");
  });

  it("passes through all config fields", () => {
    const opts = daemonOptsFromConfig("main", {
      account: "+1234567890",
      httpHost: "0.0.0.0",
      httpPort: 9090,
      receiveMode: "on-connection",
      sendReadReceipts: true,
      cliPath: "/opt/signal-cli/bin/signal-cli",
    } as never);
    expect(opts.account).toBe("+1234567890");
    expect(opts.httpHost).toBe("0.0.0.0");
    expect(opts.httpPort).toBe(9090);
    expect(opts.receiveMode).toBe("on-connection");
    expect(opts.sendReadReceipts).toBe(true);
    expect(opts.cliPath).toBe("/opt/signal-cli/bin/signal-cli");
  });

  it("does not set cliPath when not in config", () => {
    const opts = daemonOptsFromConfig("main", {
      account: "+1234567890",
    } as never);
    expect(opts.cliPath).toBeUndefined();
  });
});

// Test waitForReady with mocked fetch
describe("waitForReady", () => {
  it("resolves when health check succeeds", async () => {
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true }));
    const { waitForReady } = await import("./daemon.js");
    await waitForReady("http://localhost:8080", 5000, 100);
    // Should resolve without throwing
  });

  it("throws on timeout", async () => {
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new Error("refused")));
    const { waitForReady } = await import("./daemon.js");
    await expect(waitForReady("http://localhost:8080", 200, 50)).rejects.toThrow("not ready");
  });
});

describe("spawnDaemon", () => {
  it("is exported", async () => {
    const mod = await import("./daemon.js");
    expect(mod.spawnDaemon).toBeDefined();
    expect(typeof mod.spawnDaemon).toBe("function");
  });
});
