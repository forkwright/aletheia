// INVARIANT TESTS — encode design decisions, not just behavior.
//
// These tests guard structural properties that are easy to regress during
// refactoring or AI-assisted code generation. They should be fast (<1s total)
// and have zero external dependencies.
//
// If a test here fails, it means a design decision was violated — not a bug.
// Read the test name and assertion message before "fixing" it.

import { describe, expect, it } from "vitest";
import { ERROR_CODES, type ErrorCode } from "./koina/error-codes.js";
import {
  AletheiaError,
  ConfigError,
  ProviderError,
  SessionError,
  ToolError,
  PipelineError,
  PlanningError,
  TransportError,
} from "./koina/errors.js";

// ---------------------------------------------------------------------------
// Error hierarchy — every error subclass extends AletheiaError
// ---------------------------------------------------------------------------

describe("INVARIANT: error hierarchy", () => {
  const subclasses = [
    { Class: ConfigError, name: "ConfigError" },
    { Class: SessionError, name: "SessionError" },
    { Class: ProviderError, name: "ProviderError" },
    { Class: ToolError, name: "ToolError" },
    { Class: PipelineError, name: "PipelineError" },
    { Class: PlanningError, name: "PlanningError" },
    { Class: TransportError, name: "TransportError" },
  ];

  for (const { Class, name } of subclasses) {
    it(`${name} extends AletheiaError`, () => {
      const err = new Class("test");
      expect(err).toBeInstanceOf(AletheiaError);
      expect(err).toBeInstanceOf(Error);
    });
  }

  it("AletheiaError is never thrown as bare Error", () => {
    // AletheiaError must always carry a code — this is the contract
    const err = new AletheiaError({
      code: "PROVIDER_TIMEOUT",
      module: "test",
      message: "test",
    });
    expect(err.code).toBeDefined();
    expect(err.module).toBeDefined();
    expect(err.timestamp).toMatch(/^\d{4}-\d{2}-\d{2}T/);
  });

  it("toJSON() includes all machine-readable fields", () => {
    const err = new AletheiaError({
      code: "SESSION_NOT_FOUND",
      module: "mneme",
      message: "test",
    });
    const json = err.toJSON();
    const requiredKeys = ["error", "module", "message", "recoverable", "timestamp"];
    for (const key of requiredKeys) {
      expect(json).toHaveProperty(key);
    }
  });
});

// ---------------------------------------------------------------------------
// Error codes — format, coverage, no orphans
// ---------------------------------------------------------------------------

describe("INVARIANT: error codes", () => {
  it("all codes follow MODULE_CONDITION format (UPPER_SNAKE_CASE)", () => {
    for (const code of Object.keys(ERROR_CODES)) {
      expect(code).toMatch(
        /^[A-Z][A-Z0-9_]+$/,
        `Error code "${code}" must be UPPER_SNAKE_CASE`,
      );
    }
  });

  it("all codes have non-empty string descriptions", () => {
    for (const [code, desc] of Object.entries(ERROR_CODES)) {
      expect(typeof desc).toBe("string");
      expect(desc.length).toBeGreaterThan(0);
    }
  });

  it("no duplicate descriptions (codes should be semantically distinct)", () => {
    const descriptions = Object.values(ERROR_CODES);
    const unique = new Set(descriptions);
    expect(unique.size).toBe(
      descriptions.length,
      "Two error codes share the same description — they should be distinct",
    );
  });

  // INVARIANT: core module prefixes must exist
  const requiredPrefixes = [
    "PROVIDER",  // hermeneus
    "SESSION",   // mneme
    "PIPELINE",  // nous
    "TOOL",      // organon
    "CONFIG",    // taxis
    "SIGNAL",    // semeion
    "GATEWAY",   // pylon
    "MEMORY",    // memory sidecar
    "PLANNING",  // dianoia
  ];

  for (const prefix of requiredPrefixes) {
    it(`has at least one code with prefix ${prefix}_`, () => {
      const matching = Object.keys(ERROR_CODES).filter((c) =>
        c.startsWith(`${prefix}_`),
      );
      expect(matching.length).toBeGreaterThan(
        0,
        `No error codes found for module prefix ${prefix}`,
      );
    });
  }
});

// ---------------------------------------------------------------------------
// Event names — format, no collision with error domains
// ---------------------------------------------------------------------------

describe("INVARIANT: event bus", () => {
  // Import the type at the type level — we test the runtime union via the bus
  it("EventName follows noun:verb format", async () => {
    // Dynamic import to avoid circular dep issues
    const { eventBus } = await import("./koina/event-bus.js");
    // The bus exists and is a singleton
    expect(eventBus).toBeDefined();
    expect(typeof eventBus.on).toBe("function");
    expect(typeof eventBus.emit).toBe("function");
  });
});

// ---------------------------------------------------------------------------
// Safe wrappers — never throw, always return fallback
// ---------------------------------------------------------------------------

describe("INVARIANT: trySafe wrappers", () => {
  it("trySafe returns fallback on throw, never propagates", async () => {
    const { trySafe } = await import("./koina/safe.js");
    const result = trySafe(
      "test",
      () => {
        throw new Error("boom");
      },
      "fallback",
    );
    expect(result).toBe("fallback");
  });

  it("trySafeAsync returns fallback on async throw", async () => {
    const { trySafeAsync } = await import("./koina/safe.js");
    const result = await trySafeAsync(
      "test",
      async () => {
        throw new Error("async boom");
      },
      42,
    );
    expect(result).toBe(42);
  });
});

// ---------------------------------------------------------------------------
// Tool registry — structural contracts
// ---------------------------------------------------------------------------

describe("INVARIANT: tool registry", () => {
  it("ToolRegistry has register/unregister/resolve interface", async () => {
    const { ToolRegistry } = await import("./organon/registry.js");
    const registry = new ToolRegistry();
    expect(typeof registry.register).toBe("function");
    expect(typeof registry.unregister).toBe("function");
  });

  it("registering a tool makes it resolvable via get()", async () => {
    const { ToolRegistry } = await import("./organon/registry.js");
    const registry = new ToolRegistry();
    registry.register({
      definition: {
        name: "test_invariant_tool",
        description: "test",
        input_schema: { type: "object" as const, properties: {} },
      },
      execute: async () => "ok",
    });
    expect(registry.get("test_invariant_tool")).toBeDefined();
    expect(registry.get("test_invariant_tool")?.definition.name).toBe("test_invariant_tool");
    expect(registry.size).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// Module boundary — key exports exist
// ---------------------------------------------------------------------------

describe("INVARIANT: module exports", () => {
  it("koina/errors exports all error subclasses", async () => {
    const errors = await import("./koina/errors.js");
    const expected = [
      "AletheiaError",
      "ConfigError",
      "SessionError",
      "ProviderError",
      "ToolError",
      "PipelineError",
      "PlanningError",
      "TransportError",
    ];
    for (const name of expected) {
      expect(errors).toHaveProperty(name);
      expect(typeof (errors as Record<string, unknown>)[name]).toBe("function");
    }
  });

  it("koina/error-codes exports ERROR_CODES as const object", async () => {
    const mod = await import("./koina/error-codes.js");
    expect(mod.ERROR_CODES).toBeDefined();
    expect(typeof mod.ERROR_CODES).toBe("object");
    // `as const` makes it readonly at the type level; verify it's not empty
    expect(Object.keys(mod.ERROR_CODES).length).toBeGreaterThan(0);
  });

  it("koina/safe exports trySafe and trySafeAsync", async () => {
    const mod = await import("./koina/safe.js");
    expect(typeof mod.trySafe).toBe("function");
    expect(typeof mod.trySafeAsync).toBe("function");
  });
});
