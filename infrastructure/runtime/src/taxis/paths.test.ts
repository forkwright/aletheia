// Oikos paths module tests
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

describe("paths", () => {
  const originalEnv = { ...process.env };

  beforeEach(() => {
    vi.resetModules();
  });

  afterEach(() => {
    process.env = { ...originalEnv };
  });

  it("uses ALETHEIA_ROOT from env as instance root", async () => {
    process.env["ALETHEIA_ROOT"] = "/custom/instance";
    const { paths } = await import("./paths.js");
    expect(paths.root).toBe("/custom/instance");
  });

  it("derives tier directories from instance root", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.theke).toBe("/inst/theke");
    expect(paths.shared).toBe("/inst/shared");
    expect(paths.nous).toBe("/inst/nous");
    expect(paths.config).toBe("/inst/config");
    expect(paths.data).toBe("/inst/data");
    expect(paths.logs).toBe("/inst/logs");
  });

  it("derives shared subdirectories", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.sharedBin).toBe("/inst/shared/bin");
    expect(paths.sharedTools).toBe("/inst/shared/tools");
    expect(paths.sharedSkills).toBe("/inst/shared/skills");
    expect(paths.sharedHooks).toBe("/inst/shared/hooks");
    expect(paths.sharedTemplates).toBe("/inst/shared/templates");
    expect(paths.sharedCalibration).toBe("/inst/shared/calibration");
    expect(paths.sharedSchemas).toBe("/inst/shared/schemas");
    expect(paths.coordination).toBe("/inst/shared/coordination");
  });

  it("configDir uses ALETHEIA_CONFIG_DIR env override", async () => {
    process.env["ALETHEIA_CONFIG_DIR"] = "/custom/config";
    const { paths } = await import("./paths.js");
    expect(paths.configDir()).toBe("/custom/config");
  });

  it("configDir defaults to instance/config without env override", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    delete process.env["ALETHEIA_CONFIG_DIR"];
    const { paths } = await import("./paths.js");
    expect(paths.configDir()).toBe("/inst/config");
  });

  it("configFile joins configDir with aletheia.json", async () => {
    process.env["ALETHEIA_CONFIG_DIR"] = "/etc/aletheia";
    const { paths } = await import("./paths.js");
    expect(paths.configFile()).toBe("/etc/aletheia/aletheia.json");
  });

  it("nousDir constructs per-agent workspace path", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.nousDir("syn")).toBe("/inst/nous/syn");
  });

  it("nousFile constructs file path in agent workspace", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.nousFile("syn", "SOUL.md")).toBe("/inst/nous/syn/SOUL.md");
  });

  it("sessionsDb resolves to data/sessions.db", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.sessionsDb()).toBe("/inst/data/sessions.db");
  });

  it("credentialFile resolves to config/credentials/<provider>.json", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.credentialFile("anthropic")).toBe("/inst/config/credentials/anthropic.json");
  });

  it("credentialsDir resolves to config/credentials/", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.credentialsDir()).toBe("/inst/config/credentials");
  });

  it("planningDb resolves to data/planning.db", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.planningDb()).toBe("/inst/data/planning.db");
  });

  it("coordination paths resolve under shared/coordination/", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.tracesDir()).toBe("/inst/shared/coordination/traces");
    expect(paths.statusDir()).toBe("/inst/shared/coordination/status");
    expect(paths.evolutionDir()).toBe("/inst/shared/coordination/evolution");
    expect(paths.patchesDir()).toBe("/inst/shared/coordination/patches");
    expect(paths.prosocheDir()).toBe("/inst/shared/coordination/prosoche");
    expect(paths.memoryDir()).toBe("/inst/shared/coordination/memory");
  });

  it("authoredToolsDir resolves under shared/tools/authored", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    const { paths } = await import("./paths.js");
    expect(paths.authoredToolsDir()).toBe("/inst/shared/tools/authored");
  });

  it("pluginRoot uses ALETHEIA_PLUGIN_ROOT env override", async () => {
    process.env["ALETHEIA_PLUGIN_ROOT"] = "/custom/plugins";
    const { paths } = await import("./paths.js");
    expect(paths.pluginRoot).toBe("/custom/plugins");
  });

  it("pluginRoot defaults to shared/plugins without env override", async () => {
    process.env["ALETHEIA_ROOT"] = "/inst";
    delete process.env["ALETHEIA_PLUGIN_ROOT"];
    const { paths } = await import("./paths.js");
    expect(paths.pluginRoot).toBe("/inst/shared/plugins");
  });

  it("repoRoot and infrastructure resolve to repo-level paths", async () => {
    const { paths } = await import("./paths.js");
    expect(paths.repoRoot).toBeTruthy();
    expect(paths.infrastructure).toBe(`${paths.repoRoot}/infrastructure`);
  });
});
