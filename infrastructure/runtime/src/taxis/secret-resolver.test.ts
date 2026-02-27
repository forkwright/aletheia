// Unit tests for taxis/secret-resolver — all SecretRef resolution paths
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConfigError } from "../koina/errors.js";

vi.mock("node:fs", () => ({
  readFileSync: vi.fn(),
}));

import { readFileSync } from "node:fs";
const mockReadFile = vi.mocked(readFileSync);

import { resolveSecretRefs } from "./secret-resolver.js";
import type { AletheiaConfig } from "./schema.js";

function makeConfig(
  providerName: string,
  overrides: Record<string, unknown> = {},
  gatewayToken?: unknown,
) {
  return {
    models: {
      providers: {
        [providerName]: { baseUrl: "https://api.example.com", ...overrides },
      },
    },
    gateway: { auth: { mode: "token", token: gatewayToken } },
  } as unknown as AletheiaConfig;
}

beforeEach(() => {
  vi.resetAllMocks();
});

describe("resolveSecretRefs — inline string passthrough", () => {
  it("returns config unchanged when apiKey is a plain string", () => {
    const config = makeConfig("openai", { apiKey: "sk-inline-key" });
    const result = resolveSecretRefs(config);
    expect((result.models.providers["openai"] as Record<string, unknown>)["apiKey"]).toBe("sk-inline-key");
  });

  it("returns config unchanged when no credential fields are set", () => {
    const config = makeConfig("openai");
    expect(() => resolveSecretRefs(config)).not.toThrow();
  });
});

describe("resolveSecretRefs — env source", () => {
  const saved = process.env["TEST_KEY"];

  afterEach(() => {
    if (saved !== undefined) {
      process.env["TEST_KEY"] = saved;
    } else {
      delete process.env["TEST_KEY"];
    }
  });

  it("resolves apiKey from env var and returns plain string", () => {
    process.env["TEST_KEY"] = "sk-test-value";
    const config = makeConfig("openai", { apiKey: { source: "env", id: "TEST_KEY" } });
    const result = resolveSecretRefs(config);
    expect((result.models.providers["openai"] as Record<string, unknown>)["apiKey"]).toBe("sk-test-value");
  });

  it("throws ConfigError with CONFIG_SECRET_UNRESOLVED when env var is not set", () => {
    delete process.env["MISSING_VAR"];
    const config = makeConfig("test", { apiKey: { source: "env", id: "MISSING_VAR" } });
    let err: unknown;
    try {
      resolveSecretRefs(config);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(ConfigError);
    expect((err as ConfigError).code).toBe("CONFIG_SECRET_UNRESOLVED");
    expect((err as ConfigError).message).toContain("models.providers.test.apiKey");
    expect((err as ConfigError).message).toContain("MISSING_VAR");
  });
});

describe("resolveSecretRefs — file source", () => {
  it("resolves apiKey from file and strips trailing newline", () => {
    mockReadFile.mockReturnValue("sk-file-key\n" as unknown as Buffer);
    const config = makeConfig("anthropic", { apiKey: { source: "file", id: "/tmp/secret.txt" } });
    const result = resolveSecretRefs(config);
    expect((result.models.providers["anthropic"] as Record<string, unknown>)["apiKey"]).toBe("sk-file-key");
  });

  it("resolves apiKey from tilde-prefixed path", () => {
    mockReadFile.mockReturnValue("sk-home-key\n" as unknown as Buffer);
    const config = makeConfig("anthropic", { apiKey: { source: "file", id: "~/keys/anthropic" } });
    const result = resolveSecretRefs(config);
    expect((result.models.providers["anthropic"] as Record<string, unknown>)["apiKey"]).toBe("sk-home-key");
    expect(mockReadFile).toHaveBeenCalledWith(
      expect.stringMatching(/\/keys\/anthropic$/),
      "utf-8",
    );
  });

  it("throws ConfigError with CONFIG_SECRET_UNRESOLVED when file is not readable", () => {
    mockReadFile.mockImplementation(() => {
      const err = new Error("ENOENT: no such file or directory");
      (err as NodeJS.ErrnoException).code = "ENOENT";
      throw err;
    });
    const config = makeConfig("test", { apiKey: { source: "file", id: "/missing/key.txt" } });
    let err: unknown;
    try {
      resolveSecretRefs(config);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(ConfigError);
    expect((err as ConfigError).code).toBe("CONFIG_SECRET_UNRESOLVED");
    expect((err as ConfigError).message).toContain("file not readable");
  });

  it("throws ConfigError with CONFIG_SECRET_UNRESOLVED when file is empty (only newline)", () => {
    mockReadFile.mockReturnValue("\n" as unknown as Buffer);
    const config = makeConfig("test", { apiKey: { source: "file", id: "/tmp/empty.txt" } });
    let err: unknown;
    try {
      resolveSecretRefs(config);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(ConfigError);
    expect((err as ConfigError).code).toBe("CONFIG_SECRET_UNRESOLVED");
    expect((err as ConfigError).message).toContain("file is empty");
  });
});

describe("resolveSecretRefs — vault source stub", () => {
  it("throws ConfigError with CONFIG_SECRET_VAULT_UNSUPPORTED for vault refs", () => {
    const config = makeConfig("test", { apiKey: { source: "vault" } });
    let err: unknown;
    try {
      resolveSecretRefs(config);
    } catch (e) {
      err = e;
    }
    expect(err).toBeInstanceOf(ConfigError);
    expect((err as ConfigError).code).toBe("CONFIG_SECRET_VAULT_UNSUPPORTED");
    expect((err as ConfigError).message).toContain("Vault source is not yet supported");
    expect((err as ConfigError).message).toContain("plugin interface");
  });
});

describe("resolveSecretRefs — gateway.auth.token", () => {
  const savedGw = process.env["GW_TOKEN"];

  afterEach(() => {
    if (savedGw !== undefined) {
      process.env["GW_TOKEN"] = savedGw;
    } else {
      delete process.env["GW_TOKEN"];
    }
  });

  it("resolves gateway.auth.token from env var", () => {
    process.env["GW_TOKEN"] = "gw-secret";
    const config = makeConfig("openai", {}, { source: "env", id: "GW_TOKEN" });
    const result = resolveSecretRefs(config);
    expect((result.gateway.auth as Record<string, unknown>)["token"]).toBe("gw-secret");
  });
});

// CRED-03: applyEnv must be called before resolveSecretRefs in createRuntime() —
// this is enforced by call order in aletheia.ts, not testable in isolation here.
// The ordering is: loadConfig → applyEnv → resolveSecretRefs.
