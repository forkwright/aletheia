// Error hierarchy unit tests
import { describe, expect, it } from "vitest";
import {
  AletheiaError,
  ConfigError,
  ProviderError,
  SessionError,
  ToolError,
} from "./errors.js";
import { ERROR_CODES, type ErrorCode } from "./error-codes.js";

describe("AletheiaError", () => {
  it("constructs with all required fields", () => {
    const err = new AletheiaError({
      code: "PROVIDER_TIMEOUT",
      module: "hermeneus",
      message: "Timed out after 30s",
    });
    expect(err).toBeInstanceOf(Error);
    expect(err).toBeInstanceOf(AletheiaError);
    expect(err.code).toBe("PROVIDER_TIMEOUT");
    expect(err.module).toBe("hermeneus");
    expect(err.message).toBe("Timed out after 30s");
    expect(err.name).toBe("AletheiaError");
    expect(err.recoverable).toBe(false);
    expect(err.context).toEqual({});
    expect(err.retryAfterMs).toBeUndefined();
    expect(err.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });

  it("carries context and recoverable flag", () => {
    const err = new AletheiaError({
      code: "PROVIDER_RATE_LIMITED",
      module: "hermeneus",
      message: "Rate limited",
      context: { retryAfter: 5000, model: "opus" },
      recoverable: true,
      retryAfterMs: 5000,
    });
    expect(err.context).toEqual({ retryAfter: 5000, model: "opus" });
    expect(err.recoverable).toBe(true);
    expect(err.retryAfterMs).toBe(5000);
  });

  it("preserves cause chain", () => {
    const root = new Error("socket hang up");
    const err = new AletheiaError({
      code: "PROVIDER_TIMEOUT",
      module: "hermeneus",
      message: "Request failed",
      cause: root,
    });
    expect(err.cause).toBe(root);
  });

  it("serializes to JSON with all fields", () => {
    const err = new AletheiaError({
      code: "SESSION_NOT_FOUND",
      module: "mneme",
      message: "Missing session abc123",
      context: { sessionId: "abc123" },
    });
    const json = err.toJSON();
    expect(json["error"]).toBe("SESSION_NOT_FOUND");
    expect(json["module"]).toBe("mneme");
    expect(json["message"]).toBe("Missing session abc123");
    expect(json["context"]).toEqual({ sessionId: "abc123" });
    expect(json["recoverable"]).toBe(false);
    expect(json["timestamp"]).toBeDefined();
    expect(json["stack"]).toBeDefined();
  });

  it("works with JSON.stringify", () => {
    const err = new AletheiaError({
      code: "PROVIDER_TIMEOUT",
      module: "hermeneus",
      message: "test",
    });
    const parsed = JSON.parse(JSON.stringify(err));
    expect(parsed["error"]).toBe("PROVIDER_TIMEOUT");
    expect(parsed["module"]).toBe("hermeneus");
  });
});

describe("ConfigError", () => {
  it("defaults to CONFIG_VALIDATION_FAILED code", () => {
    const err = new ConfigError("Bad config");
    expect(err).toBeInstanceOf(AletheiaError);
    expect(err.name).toBe("ConfigError");
    expect(err.code).toBe("CONFIG_VALIDATION_FAILED");
    expect(err.module).toBe("taxis");
    expect(err.message).toBe("Bad config");
  });

  it("accepts custom code and context", () => {
    const err = new ConfigError("File not found", {
      code: "CONFIG_NOT_FOUND",
      context: { path: "/etc/aletheia.json" },
    });
    expect(err.code).toBe("CONFIG_NOT_FOUND");
    expect(err.context).toEqual({ path: "/etc/aletheia.json" });
  });
});

describe("SessionError", () => {
  it("defaults to SESSION_NOT_FOUND code", () => {
    const err = new SessionError("No session");
    expect(err).toBeInstanceOf(AletheiaError);
    expect(err.name).toBe("SessionError");
    expect(err.code).toBe("SESSION_NOT_FOUND");
    expect(err.module).toBe("mneme");
  });

  it("preserves cause", () => {
    const cause = new Error("SQL error");
    const err = new SessionError("DB failure", { cause });
    expect(err.cause).toBe(cause);
  });
});

describe("ProviderError", () => {
  it("defaults to PROVIDER_TIMEOUT code", () => {
    const err = new ProviderError("Timeout");
    expect(err.code).toBe("PROVIDER_TIMEOUT");
    expect(err.module).toBe("hermeneus");
    expect(err.name).toBe("ProviderError");
  });

  it("carries recoverable + retryAfterMs", () => {
    const err = new ProviderError("Rate limited", {
      code: "PROVIDER_RATE_LIMITED",
      recoverable: true,
      retryAfterMs: 10000,
    });
    expect(err.recoverable).toBe(true);
    expect(err.retryAfterMs).toBe(10000);
  });
});

describe("ToolError", () => {
  it("defaults to TOOL_EXECUTION_FAILED code", () => {
    const err = new ToolError("bash failed");
    expect(err.code).toBe("TOOL_EXECUTION_FAILED");
    expect(err.module).toBe("organon");
    expect(err.name).toBe("ToolError");
  });

  it("accepts context for debugging", () => {
    const err = new ToolError("exec timeout", {
      code: "EXEC_TIMEOUT",
      context: { command: "sleep 999", timeoutMs: 5000 },
    });
    expect(err.code).toBe("EXEC_TIMEOUT");
    expect(err.context).toEqual({ command: "sleep 999", timeoutMs: 5000 });
  });
});

describe("ERROR_CODES registry", () => {
  it("has descriptions for all codes", () => {
    for (const [code, desc] of Object.entries(ERROR_CODES)) {
      expect(desc).toBeTruthy();
      expect(typeof desc).toBe("string");
      expect(code).toMatch(/^[A-Z][A-Z0-9_]+$/);
    }
  });

  it("codes are usable as ErrorCode type", () => {
    const code: ErrorCode = "PROVIDER_TIMEOUT";
    expect(ERROR_CODES[code]).toBe("API call timed out");
  });

  it("covers all modules", () => {
    const codes = Object.keys(ERROR_CODES);
    const prefixes = new Set(codes.map((c) => c.split("_")[0]!));
    expect(prefixes).toContain("PROVIDER");
    expect(prefixes).toContain("SESSION");
    expect(prefixes).toContain("CONFIG");
    expect(prefixes).toContain("TOOL");
    expect(prefixes).toContain("SIGNAL");
    expect(prefixes).toContain("PLUGIN");
    expect(prefixes).toContain("GATEWAY");
    expect(prefixes).toContain("MEMORY");
  });
});
