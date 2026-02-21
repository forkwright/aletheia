// Hook system tests
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  substituteTemplateVars,
  parseSimpleYaml,
  loadHookDefinitions,
  executeShellHook,
  registerHooks,
  type HookDefinition,
} from "./hooks.js";

vi.mock("./logger.js", () => ({
  createLogger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
  }),
}));

// --- Template substitution ---

describe("substituteTemplateVars", () => {
  it("replaces simple variables", () => {
    const result = substituteTemplateVars(
      "Session {{sessionId}} for {{nousId}}",
      { sessionId: "ses_123", nousId: "syn" },
    );
    expect(result).toBe("Session ses_123 for syn");
  });

  it("replaces nested dot-notation variables", () => {
    const result = substituteTemplateVars(
      "{{session.id}} at {{session.timestamp}}",
      { session: { id: "ses_42", timestamp: "2026-02-22" } },
    );
    expect(result).toBe("ses_42 at 2026-02-22");
  });

  it("replaces missing variables with empty string", () => {
    const result = substituteTemplateVars(
      "Hello {{name}}, your {{missing}} is ready",
      { name: "Cody" },
    );
    expect(result).toBe("Hello Cody, your  is ready");
  });

  it("serializes object values as JSON", () => {
    const result = substituteTemplateVars(
      "Data: {{payload}}",
      { payload: { a: 1, b: 2 } },
    );
    expect(result).toBe('Data: {"a":1,"b":2}');
  });

  it("handles numeric values", () => {
    const result = substituteTemplateVars(
      "Tokens: {{tokens}}",
      { tokens: 1500 },
    );
    expect(result).toBe("Tokens: 1500");
  });

  it("handles boolean values", () => {
    const result = substituteTemplateVars(
      "Active: {{active}}",
      { active: true },
    );
    expect(result).toBe("Active: true");
  });

  it("returns input unchanged when no template vars present", () => {
    const result = substituteTemplateVars("no vars here", { foo: "bar" });
    expect(result).toBe("no vars here");
  });

  it("handles null/undefined in nested path gracefully", () => {
    const result = substituteTemplateVars(
      "Value: {{deep.nested.path}}",
      { deep: null },
    );
    expect(result).toBe("Value: ");
  });
});

// --- Simple YAML parser ---

describe("parseSimpleYaml", () => {
  it("parses flat key-value pairs", () => {
    const yaml = `
name: my-hook
event: turn:after
enabled: true
`;
    const result = parseSimpleYaml(yaml);
    expect(result).toEqual({
      name: "my-hook",
      event: "turn:after",
      enabled: true,
    });
  });

  it("parses nested objects", () => {
    const yaml = `
name: test-hook
event: distill:after
handler:
  type: shell
  command: /usr/bin/echo
  timeout: 10s
  failAction: warn
`;
    const result = parseSimpleYaml(yaml);
    expect(result).toEqual({
      name: "test-hook",
      event: "distill:after",
      handler: {
        type: "shell",
        command: "/usr/bin/echo",
        timeout: "10s",
        failAction: "warn",
      },
    });
  });

  it("parses inline arrays", () => {
    const yaml = `
name: filtered-hook
nousFilter: [syn, demiurge]
`;
    const result = parseSimpleYaml(yaml);
    expect(result).toEqual({
      name: "filtered-hook",
      nousFilter: ["syn", "demiurge"],
    });
  });

  it("parses inline arrays with quoted strings", () => {
    const yaml = `
name: args-hook
handler:
  command: /bin/test
  args: ["{{sessionId}}", "{{nousId}}"]
`;
    const result = parseSimpleYaml(yaml);
    expect(result?.handler).toEqual({
      command: "/bin/test",
      args: ["{{sessionId}}", "{{nousId}}"],
    });
  });

  it("strips comments", () => {
    const yaml = `
name: commented  # this is a comment
event: boot:ready  # another comment
`;
    const result = parseSimpleYaml(yaml);
    expect(result).toEqual({
      name: "commented",
      event: "boot:ready",
    });
  });

  it("handles numbers and booleans", () => {
    const yaml = `
count: 42
ratio: 3.14
active: true
disabled: false
`;
    const result = parseSimpleYaml(yaml);
    expect(result).toEqual({
      count: 42,
      ratio: 3.14,
      active: true,
      disabled: false,
    });
  });

  it("handles quoted strings", () => {
    const yaml = `
name: "my hook"
path: '/usr/local/bin'
`;
    const result = parseSimpleYaml(yaml);
    expect(result).toEqual({
      name: "my hook",
      path: "/usr/local/bin",
    });
  });

  it("returns null for empty content", () => {
    expect(parseSimpleYaml("")).toBeNull();
    expect(parseSimpleYaml("  \n  \n")).toBeNull();
    expect(parseSimpleYaml("# just comments")).toBeNull();
  });
});

// --- Shell execution ---

describe("executeShellHook", () => {
  it("executes a simple command and captures output", async () => {
    const hook: HookDefinition = {
      name: "test-echo",
      event: "boot:ready",
      handler: {
        type: "shell",
        command: "/bin/echo",
        args: ["hello", "world"],
        timeout: "5s",
        failAction: "warn",
        env: {},
      },
      enabled: true,
    };

    const result = await executeShellHook(hook, { timestamp: Date.now() });
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toBe("hello world");
    expect(result.timedOut).toBe(false);
    expect(result.durationMs).toBeGreaterThanOrEqual(0);
  });

  it("captures non-zero exit codes", async () => {
    const hook: HookDefinition = {
      name: "test-fail",
      event: "boot:ready",
      handler: {
        type: "shell",
        command: "/bin/sh",
        args: ["-c", "exit 2"],
        timeout: "5s",
        failAction: "warn",
        env: {},
      },
      enabled: true,
    };

    const result = await executeShellHook(hook, {});
    expect(result.exitCode).toBe(2);
    expect(result.timedOut).toBe(false);
  });

  it("substitutes template variables in args", async () => {
    const hook: HookDefinition = {
      name: "test-vars",
      event: "turn:after",
      handler: {
        type: "shell",
        command: "/bin/echo",
        args: ["{{nousId}}", "{{sessionId}}"],
        timeout: "5s",
        failAction: "warn",
        env: {},
      },
      enabled: true,
    };

    const result = await executeShellHook(hook, {
      nousId: "syn",
      sessionId: "ses_42",
    });
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toBe("syn ses_42");
  });

  it("passes event payload as JSON on stdin", async () => {
    const hook: HookDefinition = {
      name: "test-stdin",
      event: "turn:after",
      handler: {
        type: "shell",
        command: "/bin/cat",
        args: [],
        timeout: "5s",
        failAction: "warn",
        env: {},
      },
      enabled: true,
    };

    const payload = { nousId: "syn", tokens: 1500 };
    const result = await executeShellHook(hook, payload);
    expect(result.exitCode).toBe(0);
    expect(JSON.parse(result.stdout)).toEqual(payload);
  });

  it("sets ALETHEIA_HOOK_NAME and ALETHEIA_HOOK_EVENT env vars", async () => {
    const hook: HookDefinition = {
      name: "test-env",
      event: "distill:after",
      handler: {
        type: "shell",
        command: "/bin/sh",
        args: ["-c", "echo $ALETHEIA_HOOK_NAME:$ALETHEIA_HOOK_EVENT"],
        timeout: "5s",
        failAction: "warn",
        env: {},
      },
      enabled: true,
    };

    const result = await executeShellHook(hook, {});
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toBe("test-env:distill:after");
  });

  it("passes custom env vars", async () => {
    const hook: HookDefinition = {
      name: "test-custom-env",
      event: "boot:ready",
      handler: {
        type: "shell",
        command: "/bin/sh",
        args: ["-c", "echo $MY_VAR"],
        timeout: "5s",
        failAction: "warn",
        env: { MY_VAR: "hello" },
      },
      enabled: true,
    };

    const result = await executeShellHook(hook, {});
    expect(result.exitCode).toBe(0);
    expect(result.stdout).toBe("hello");
  });

  it("times out long-running commands", async () => {
    const hook: HookDefinition = {
      name: "test-timeout",
      event: "boot:ready",
      handler: {
        type: "shell",
        command: "/bin/sleep",
        args: ["10"],
        timeout: "100ms",
        failAction: "warn",
        env: {},
      },
      enabled: true,
    };

    const result = await executeShellHook(hook, {});
    expect(result.timedOut).toBe(true);
    expect(result.exitCode).toBeNull();
  });

  it("captures stderr on failure", async () => {
    const hook: HookDefinition = {
      name: "test-stderr",
      event: "boot:ready",
      handler: {
        type: "shell",
        command: "/bin/sh",
        args: ["-c", "echo 'error msg' >&2; exit 1"],
        timeout: "5s",
        failAction: "warn",
        env: {},
      },
      enabled: true,
    };

    const result = await executeShellHook(hook, {});
    expect(result.exitCode).toBe(1);
    expect(result.stderr).toBe("error msg");
  });
});

// --- YAML loading from directory ---

describe("loadHookDefinitions", () => {
  it("returns empty array for non-existent directory", () => {
    const hooks = loadHookDefinitions("/nonexistent/path/hooks");
    expect(hooks).toEqual([]);
  });

  it("loads valid hook YAML from a real directory", () => {
    const tmpDir = mkdtempSync(join(tmpdir(), "hooks-test-"));
    try {
      writeFileSync(
        join(tmpDir, "test-hook.yaml"),
        `name: test-hook
event: turn:after
handler:
  type: shell
  command: /bin/echo
  args: ["hello"]
  timeout: 5s
  failAction: warn
`,
      );

      const hooks = loadHookDefinitions(tmpDir);
      expect(hooks).toHaveLength(1);
      expect(hooks[0]!.name).toBe("test-hook");
      expect(hooks[0]!.event).toBe("turn:after");
      expect(hooks[0]!.handler.command).toBe("/bin/echo");
    } finally {
      rmSync(tmpDir, { recursive: true });
    }
  });

  it("skips disabled hooks", () => {
    const tmpDir = mkdtempSync(join(tmpdir(), "hooks-test-"));
    try {
      writeFileSync(
        join(tmpDir, "disabled.yaml"),
        `name: disabled-hook
event: boot:ready
enabled: false
handler:
  type: shell
  command: /bin/echo
`,
      );

      const hooks = loadHookDefinitions(tmpDir);
      expect(hooks).toHaveLength(0);
    } finally {
      rmSync(tmpDir, { recursive: true });
    }
  });

  it("skips hooks with invalid event names", () => {
    const tmpDir = mkdtempSync(join(tmpdir(), "hooks-test-"));
    try {
      writeFileSync(
        join(tmpDir, "bad-event.yaml"),
        `name: bad-event-hook
event: nonexistent:event
handler:
  type: shell
  command: /bin/echo
`,
      );

      const hooks = loadHookDefinitions(tmpDir);
      expect(hooks).toHaveLength(0);
    } finally {
      rmSync(tmpDir, { recursive: true });
    }
  });

  it("skips hooks with disallowed command extensions", () => {
    const tmpDir = mkdtempSync(join(tmpdir(), "hooks-test-"));
    try {
      writeFileSync(
        join(tmpDir, "bad-ext.yaml"),
        `name: bad-ext-hook
event: boot:ready
handler:
  type: shell
  command: /tmp/evil.exe
`,
      );

      const hooks = loadHookDefinitions(tmpDir);
      expect(hooks).toHaveLength(0);
    } finally {
      rmSync(tmpDir, { recursive: true });
    }
  });

  it("loads multiple hooks from multiple files", () => {
    const tmpDir = mkdtempSync(join(tmpdir(), "hooks-test-"));
    try {
      writeFileSync(
        join(tmpDir, "hook-a.yaml"),
        `name: hook-a
event: turn:before
handler:
  type: shell
  command: /bin/echo
`,
      );
      writeFileSync(
        join(tmpDir, "hook-b.yml"),
        `name: hook-b
event: distill:after
handler:
  type: shell
  command: /bin/echo
`,
      );

      const hooks = loadHookDefinitions(tmpDir);
      expect(hooks).toHaveLength(2);
      const names = hooks.map((h) => h.name).sort();
      expect(names).toEqual(["hook-a", "hook-b"]);
    } finally {
      rmSync(tmpDir, { recursive: true });
    }
  });

  it("ignores non-yaml files", () => {
    const tmpDir = mkdtempSync(join(tmpdir(), "hooks-test-"));
    try {
      writeFileSync(join(tmpDir, "README.md"), "# not a hook");
      writeFileSync(join(tmpDir, "notes.txt"), "not a hook either");
      writeFileSync(
        join(tmpDir, "real.yaml"),
        `name: real-hook
event: boot:ready
handler:
  type: shell
  command: /bin/echo
`,
      );

      const hooks = loadHookDefinitions(tmpDir);
      expect(hooks).toHaveLength(1);
      expect(hooks[0]!.name).toBe("real-hook");
    } finally {
      rmSync(tmpDir, { recursive: true });
    }
  });
});

// --- Hook registration ---

describe("registerHooks", () => {
  it("returns empty hooks array for non-existent directory", () => {
    const registry = registerHooks("/nonexistent/path/hooks");
    expect(registry.hooks).toEqual([]);
    registry.teardown();
  });

  it("registers hooks onto event bus and teardown removes them", async () => {
    const tmpDir = mkdtempSync(join(tmpdir(), "hooks-test-"));
    try {
      writeFileSync(
        join(tmpDir, "test.yaml"),
        `name: bus-test
event: boot:ready
handler:
  type: shell
  command: /bin/echo
  args: ["test"]
`,
      );

      const { eventBus: bus } = await import("./event-bus.js");
      const countBefore = bus.listenerCount("boot:ready");

      const registry = registerHooks(tmpDir);
      expect(registry.hooks).toHaveLength(1);
      expect(bus.listenerCount("boot:ready")).toBe(countBefore + 1);

      registry.teardown();
      expect(bus.listenerCount("boot:ready")).toBe(countBefore);
    } finally {
      rmSync(tmpDir, { recursive: true });
    }
  });
});
