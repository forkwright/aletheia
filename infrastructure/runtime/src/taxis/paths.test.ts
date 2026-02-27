// Paths module tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConfigError } from "../koina/errors.js";

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

describe("anchor-based paths", () => {
  beforeEach(() => {
    vi.resetModules();
  });

  it("nousSharedDir() and deployDir() return values set by initPaths()", async () => {
    const { initPaths, nousSharedDir, deployDir } = await import("./paths.js");
    initPaths({ nousDir: "/custom/nous", deployDir: "/custom/deploy" });
    expect(nousSharedDir()).toBe("/custom/nous");
    expect(deployDir()).toBe("/custom/deploy");
  });

  it("nousAgentDir() returns join of nousSharedDir and nousId", async () => {
    const { initPaths, nousAgentDir } = await import("./paths.js");
    initPaths({ nousDir: "/custom/nous", deployDir: "/custom/deploy" });
    expect(nousAgentDir("syn")).toBe("/custom/nous/syn");
  });

  it("nousSharedDir() throws ConfigError with CONFIG_ANCHOR_NOT_FOUND before initPaths()", async () => {
    const { nousSharedDir } = await import("./paths.js");
    let caught: ConfigError | undefined;
    try {
      nousSharedDir();
    } catch (e) {
      caught = e as ConfigError;
    }
    expect(caught).toBeDefined();
    expect(caught?.code).toBe("CONFIG_ANCHOR_NOT_FOUND");
  });

  it("deployDir() throws ConfigError with CONFIG_ANCHOR_NOT_FOUND before initPaths()", async () => {
    const { deployDir } = await import("./paths.js");
    let caught: ConfigError | undefined;
    try {
      deployDir();
    } catch (e) {
      caught = e as ConfigError;
    }
    expect(caught).toBeDefined();
    expect(caught?.code).toBe("CONFIG_ANCHOR_NOT_FOUND");
  });
});
