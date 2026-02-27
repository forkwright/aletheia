// Tests for Agora CLI — channel management
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { existsSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

// Test helpers
function createTempConfig(channels?: Record<string, unknown>): { dir: string; path: string } {
  const dir = join(tmpdir(), `agora-cli-test-${Date.now()}-${Math.random().toString(36).slice(2)}`);
  mkdirSync(dir, { recursive: true });
  const path = join(dir, "aletheia.json");
  const config: Record<string, unknown> = {
    models: { primary: { provider: "anthropic", model: "test" } },
    agents: { list: [] },
    bindings: [],
  };
  if (channels) config["channels"] = channels;
  writeFileSync(path, JSON.stringify(config, null, 2));
  return { dir, path };
}

function readConfig(path: string): Record<string, unknown> {
  return JSON.parse(readFileSync(path, "utf-8")) as Record<string, unknown>;
}

describe("agora/cli", () => {
  const consoleSpy = { log: vi.fn(), error: vi.fn() };
  const origLog = console.log;
  const origError = console.error;

  beforeEach(() => {
    consoleSpy.log = vi.fn();
    consoleSpy.error = vi.fn();
    console.log = consoleSpy.log;
    console.error = consoleSpy.error;
  });

  afterEach(() => {
    console.log = origLog;
    console.error = origError;
  });

  describe("channelList", () => {
    it("shows signal and slack status when both configured", async () => {
      const { path } = createTempConfig({
        signal: { enabled: true, accounts: { default: { account: "+1" } } },
        slack: { enabled: true, mode: "socket", appToken: "xapp-1", botToken: "xoxb-1" },
      });
      const { channelList } = await import("./cli.js");
      channelList(path);

      const output = consoleSpy.log.mock.calls.map((c: unknown[]) => c[0]).join("\n");
      expect(output).toContain("signal");
      expect(output).toContain("✓ enabled");
      expect(output).toContain("slack");
    });

    it("shows not configured for missing channels", async () => {
      const { path } = createTempConfig({});
      const { channelList } = await import("./cli.js");
      channelList(path);

      const output = consoleSpy.log.mock.calls.map((c: unknown[]) => c[0]).join("\n");
      expect(output).toContain("slack");
      expect(output).toContain("not configured");
    });
  });

  describe("channelRemove", () => {
    it("removes slack config from file", async () => {
      const { path } = createTempConfig({
        signal: { enabled: true, accounts: {} },
        slack: { enabled: true },
      });
      const { channelRemove } = await import("./cli.js");
      channelRemove("slack", path);

      const config = readConfig(path);
      const channels = config["channels"] as Record<string, unknown>;
      expect(channels["slack"]).toBeUndefined();
      expect(channels["signal"]).toBeDefined();
    });

    it("refuses to remove signal", async () => {
      const { path } = createTempConfig({
        signal: { enabled: true, accounts: {} },
      });

      const mockExit = vi.spyOn(process, "exit").mockImplementation(() => { throw new Error("exit"); });
      const { channelRemove } = await import("./cli.js");

      expect(() => channelRemove("signal", path)).toThrow("exit");
      mockExit.mockRestore();
    });
  });

  describe("isSupportedChannel", () => {
    it("recognizes slack", async () => {
      const { isSupportedChannel } = await import("./cli.js");
      expect(isSupportedChannel("slack")).toBe(true);
    });

    it("rejects unknown channels", async () => {
      const { isSupportedChannel } = await import("./cli.js");
      expect(isSupportedChannel("discord")).toBe(false);
      expect(isSupportedChannel("signal")).toBe(false); // signal is built-in, not addable
    });
  });

  describe("listSupportedChannels", () => {
    it("returns supported channel list", async () => {
      const { listSupportedChannels } = await import("./cli.js");
      expect(listSupportedChannels()).toEqual(["slack"]);
    });
  });
});
