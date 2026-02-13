// Config loading unit tests
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { loadConfig, resolveNous, resolveModel, resolveWorkspace, resolveDefaultNous, allNousIds } from "./loader.js";
import { ConfigError } from "../koina/errors.js";
import type { AletheiaConfig, NousConfig } from "./schema.js";

// Mock readJson — loader depends on koina/fs.readJson
vi.mock("../koina/fs.js", () => ({
  readJson: vi.fn(),
}));

// Mock paths — loader imports paths.configFile()
vi.mock("./paths.js", () => ({
  paths: {
    configFile: () => "/test/aletheia.json",
    nousDir: (id: string) => `/test/nous/${id}`,
  },
}));

import { readJson } from "../koina/fs.js";
const mockReadJson = vi.mocked(readJson);

// Minimal valid config object
function minimalConfig(overrides?: Record<string, unknown>): Record<string, unknown> {
  return {
    agents: {
      list: [
        { id: "syn", workspace: "/nous/syn" },
      ],
    },
    ...overrides,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("loadConfig", () => {
  it("loads and parses valid config", () => {
    mockReadJson.mockReturnValue(minimalConfig());
    const config = loadConfig("/test/aletheia.json");
    expect(config.agents.list.length).toBe(1);
    expect(config.agents.list[0]!.id).toBe("syn");
  });

  it("throws CONFIG_NOT_FOUND when file missing", () => {
    mockReadJson.mockReturnValue(null);
    expect(() => loadConfig("/nope.json")).toThrow(ConfigError);
    try {
      loadConfig("/nope.json");
    } catch (err) {
      expect(err).toBeInstanceOf(ConfigError);
      expect((err as ConfigError).code).toBe("CONFIG_NOT_FOUND");
      expect((err as ConfigError).context).toHaveProperty("path");
    }
  });

  it("throws CONFIG_VALIDATION_FAILED for invalid schema", () => {
    // agents.list requires array of objects with id+workspace
    mockReadJson.mockReturnValue({ agents: { list: [{ bad: true }] } });
    expect(() => loadConfig()).toThrow(ConfigError);
    try {
      loadConfig();
    } catch (err) {
      expect((err as ConfigError).code).toBe("CONFIG_VALIDATION_FAILED");
    }
  });

  it("applies defaults for missing optional sections", () => {
    mockReadJson.mockReturnValue(minimalConfig());
    const config = loadConfig();
    expect(config.gateway.port).toBe(18789);
    expect(config.session.scope).toBe("per-sender");
    expect(config.cron.enabled).toBe(true);
    expect(config.plugins.enabled).toBe(true);
    expect(config.agents.defaults.contextTokens).toBe(200000);
    expect(config.agents.defaults.maxOutputTokens).toBe(16384);
    expect(config.agents.defaults.bootstrapMaxTokens).toBe(40000);
  });

  it("uses provided configPath over default", () => {
    mockReadJson.mockReturnValue(minimalConfig());
    loadConfig("/custom/path.json");
    expect(mockReadJson).toHaveBeenCalledWith("/custom/path.json");
  });

  it("preserves passthrough fields", () => {
    mockReadJson.mockReturnValue(minimalConfig({ customField: "preserved" }));
    const config = loadConfig();
    expect((config as Record<string, unknown>)["customField"]).toBe("preserved");
  });

  it("handles multiple agents", () => {
    mockReadJson.mockReturnValue({
      agents: {
        list: [
          { id: "syn", workspace: "/nous/syn", default: true },
          { id: "chiron", workspace: "/nous/chiron" },
          { id: "syl", workspace: "/nous/syl" },
        ],
      },
    });
    const config = loadConfig();
    expect(config.agents.list.length).toBe(3);
  });

  it("parses bindings", () => {
    mockReadJson.mockReturnValue({
      ...minimalConfig(),
      bindings: [
        { agentId: "syn", match: { channel: "signal", peer: { kind: "user", id: "+123" } } },
      ],
    });
    const config = loadConfig();
    expect(config.bindings.length).toBe(1);
    expect(config.bindings[0]!.agentId).toBe("syn");
  });

  it("handles backwards-compat bootstrapMaxChars → bootstrapMaxTokens", () => {
    mockReadJson.mockReturnValue({
      agents: {
        defaults: { bootstrapMaxChars: 50000 },
        list: [{ id: "syn", workspace: "/nous/syn" }],
      },
    });
    const config = loadConfig();
    expect(config.agents.defaults.bootstrapMaxTokens).toBe(50000);
  });
});

describe("resolveNous", () => {
  let config: AletheiaConfig;

  beforeEach(() => {
    mockReadJson.mockReturnValue(minimalConfig({
      agents: {
        list: [
          { id: "syn", workspace: "/nous/syn", default: true },
          { id: "chiron", workspace: "/nous/chiron" },
        ],
      },
    }));
    config = loadConfig();
  });

  it("finds agent by ID", () => {
    const nous = resolveNous(config, "syn");
    expect(nous).toBeDefined();
    expect(nous!.id).toBe("syn");
  });

  it("returns undefined for unknown ID", () => {
    expect(resolveNous(config, "ghost")).toBeUndefined();
  });
});

describe("resolveDefaultNous", () => {
  it("returns agent with default: true", () => {
    mockReadJson.mockReturnValue({
      agents: {
        list: [
          { id: "syl", workspace: "/nous/syl" },
          { id: "syn", workspace: "/nous/syn", default: true },
        ],
      },
    });
    const config = loadConfig();
    const def = resolveDefaultNous(config);
    expect(def?.id).toBe("syn");
  });

  it("falls back to first agent when no default", () => {
    mockReadJson.mockReturnValue({
      agents: {
        list: [
          { id: "syl", workspace: "/nous/syl" },
          { id: "chiron", workspace: "/nous/chiron" },
        ],
      },
    });
    const config = loadConfig();
    const def = resolveDefaultNous(config);
    expect(def?.id).toBe("syl");
  });
});

describe("resolveModel", () => {
  let config: AletheiaConfig;

  beforeEach(() => {
    mockReadJson.mockReturnValue(minimalConfig());
    config = loadConfig();
  });

  it("uses global default when no per-agent model", () => {
    const nous = config.agents.list[0];
    const model = resolveModel(config, nous);
    expect(model).toBe("claude-opus-4-6");
  });

  it("uses per-agent string model", () => {
    mockReadJson.mockReturnValue({
      agents: {
        list: [{ id: "syn", workspace: "/nous/syn", model: "claude-haiku" }],
      },
    });
    const c = loadConfig();
    const model = resolveModel(c, c.agents.list[0]);
    expect(model).toBe("claude-haiku");
  });

  it("uses per-agent object model primary", () => {
    mockReadJson.mockReturnValue({
      agents: {
        list: [{
          id: "syn",
          workspace: "/nous/syn",
          model: { primary: "claude-sonnet", fallbacks: ["claude-haiku"] },
        }],
      },
    });
    const c = loadConfig();
    const model = resolveModel(c, c.agents.list[0]);
    expect(model).toBe("claude-sonnet");
  });

  it("uses global default when no nous provided", () => {
    expect(resolveModel(config)).toBe("claude-opus-4-6");
  });
});

describe("resolveWorkspace", () => {
  it("uses agent workspace when specified", () => {
    mockReadJson.mockReturnValue(minimalConfig());
    const config = loadConfig();
    const ws = resolveWorkspace(config, config.agents.list[0]!);
    expect(ws).toBe("/nous/syn");
  });
});

describe("allNousIds", () => {
  it("returns list of all agent IDs", () => {
    mockReadJson.mockReturnValue({
      agents: {
        list: [
          { id: "syn", workspace: "/nous/syn" },
          { id: "chiron", workspace: "/nous/chiron" },
        ],
      },
    });
    const config = loadConfig();
    expect(allNousIds(config)).toEqual(["syn", "chiron"]);
  });
});
