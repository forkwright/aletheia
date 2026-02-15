// Paths module tests
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

describe("paths", () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    vi.resetModules();
  });

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it("uses ALETHEIA_ROOT from env", async () => {
    process.env["ALETHEIA_ROOT"] = "/custom/root";
    const { paths } = await import("./paths.js");
    expect(paths.root).toBe("/custom/root");
    expect(paths.nous).toBe("/custom/root/nous");
    expect(paths.shared).toBe("/custom/root/shared");
  });

  it("defaults ALETHEIA_ROOT to /mnt/ssd/aletheia", async () => {
    delete process.env["ALETHEIA_ROOT"];
    const { paths } = await import("./paths.js");
    expect(paths.root).toBe("/mnt/ssd/aletheia");
  });

  it("configDir uses ALETHEIA_CONFIG_DIR env", async () => {
    process.env["ALETHEIA_CONFIG_DIR"] = "/custom/config";
    const { paths } = await import("./paths.js");
    expect(paths.configDir()).toBe("/custom/config");
  });

  it("configFile joins configDir with aletheia.json", async () => {
    process.env["ALETHEIA_CONFIG_DIR"] = "/etc/aletheia";
    const { paths } = await import("./paths.js");
    expect(paths.configFile()).toBe("/etc/aletheia/aletheia.json");
  });

  it("nousDir constructs agent workspace path", async () => {
    delete process.env["ALETHEIA_ROOT"];
    const { paths } = await import("./paths.js");
    expect(paths.nousDir("syn")).toBe("/mnt/ssd/aletheia/nous/syn");
  });

  it("nousFile constructs file path in agent workspace", async () => {
    delete process.env["ALETHEIA_ROOT"];
    const { paths } = await import("./paths.js");
    expect(paths.nousFile("syn", "SOUL.md")).toBe("/mnt/ssd/aletheia/nous/syn/SOUL.md");
  });

  it("sessionsDb joins configDir with sessions.db", async () => {
    process.env["ALETHEIA_CONFIG_DIR"] = "/var/aletheia";
    const { paths } = await import("./paths.js");
    expect(paths.sessionsDb()).toBe("/var/aletheia/sessions.db");
  });

  it("static paths are consistent", async () => {
    delete process.env["ALETHEIA_ROOT"];
    const { paths } = await import("./paths.js");
    expect(paths.sharedBin).toBe("/mnt/ssd/aletheia/shared/bin");
    expect(paths.sharedConfig).toBe("/mnt/ssd/aletheia/shared/config");
    expect(paths.sharedMemory).toBe("/mnt/ssd/aletheia/shared/memory");
    expect(paths.infrastructure).toBe("/mnt/ssd/aletheia/infrastructure");
  });
});
