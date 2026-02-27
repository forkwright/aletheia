// Diagnostics tests
import { describe, expect, it, vi, beforeEach } from "vitest";
import { applyFixes, type DiagnosticResult, formatResults } from "./diagnostics.js";

describe("formatResults", () => {
  it("formats ok results with + icon", () => {
    const results: DiagnosticResult[] = [
      { name: "test_check", status: "ok", message: "All good" },
    ];
    const output = formatResults(results);
    expect(output).toContain("+ test_check: All good");
    expect(output).toContain("All checks passed");
  });

  it("formats error results with X icon", () => {
    const results: DiagnosticResult[] = [
      { name: "broken", status: "error", message: "Something failed" },
    ];
    const output = formatResults(results);
    expect(output).toContain("X broken: Something failed");
    expect(output).toContain("1 error(s)");
  });

  it("formats warn results with ! icon", () => {
    const results: DiagnosticResult[] = [
      { name: "maybe", status: "warn", message: "Something iffy" },
    ];
    const output = formatResults(results);
    expect(output).toContain("! maybe: Something iffy");
    expect(output).toContain("1 warning(s)");
  });

  it("shows fixable count", () => {
    const results: DiagnosticResult[] = [
      {
        name: "fixable",
        status: "warn",
        message: "Can fix",
        fix: { description: "Do the thing", action: () => { /* no-op test stub */ } },
      },
    ];
    const output = formatResults(results, true);
    expect(output).toContain("fix: Do the thing");
    expect(output).toContain("1 fixable");
  });
});

describe("applyFixes", () => {
  it("applies fix actions and counts them", () => {
    const fn = vi.fn();
    const results: DiagnosticResult[] = [
      { name: "ok_check", status: "ok", message: "fine" },
      {
        name: "fixable",
        status: "warn",
        message: "needs fix",
        fix: { description: "fix it", action: fn },
      },
    ];
    const { applied, failed } = applyFixes(results);
    expect(applied).toBe(1);
    expect(failed).toHaveLength(0);
    expect(fn).toHaveBeenCalledOnce();
  });

  it("catches fix errors and reports them", () => {
    const results: DiagnosticResult[] = [
      {
        name: "bad_fix",
        status: "error",
        message: "broken",
        fix: {
          description: "try fix",
          action: () => { throw new Error("EACCES"); },
        },
      },
    ];
    const { applied, failed } = applyFixes(results);
    expect(applied).toBe(0);
    expect(failed).toHaveLength(1);
    expect(failed[0]).toContain("EACCES");
  });

  it("skips results without fix", () => {
    const results: DiagnosticResult[] = [
      { name: "no_fix", status: "error", message: "manual only" },
    ];
    const { applied, failed } = applyFixes(results);
    expect(applied).toBe(0);
    expect(failed).toHaveLength(0);
  });
});

describe("checkDeployConfig", () => {
  beforeEach(() => vi.resetModules());

  it("returns warn when anchor.json is absent (readJson returns null)", async () => {
    vi.doMock("../koina/fs.js", () => ({ readJson: vi.fn().mockReturnValue(null) }));
    const { checkDeployConfig } = await import("./diagnostics.js");
    const result = checkDeployConfig(null);
    expect(result.name).toBe("deploy_config");
    expect(result.status).toBe("warn");
    expect(result.message).toContain("anchor.json not found");
  });

  it("returns ok when loadConfig succeeds", async () => {
    vi.doMock("../koina/fs.js", () => ({
      readJson: vi.fn().mockReturnValue({ deployDir: "/fake/deploy" }),
    }));
    vi.doMock("../taxis/loader.js", () => ({
      loadConfig: vi.fn().mockReturnValue({}),
    }));
    const { checkDeployConfig } = await import("./diagnostics.js");
    const result = checkDeployConfig(null);
    expect(result.name).toBe("deploy_config");
    expect(result.status).toBe("ok");
    expect(result.message).toContain("aletheia.json valid");
  });

  it("returns error when loadConfig throws (invalid config)", async () => {
    vi.doMock("../koina/fs.js", () => ({
      readJson: vi.fn().mockReturnValue({ deployDir: "/fake/deploy" }),
    }));
    vi.doMock("../taxis/loader.js", () => ({
      loadConfig: vi.fn().mockImplementation(() => {
        throw new Error("agents.list[0].id: Required");
      }),
    }));
    const { checkDeployConfig } = await import("./diagnostics.js");
    const result = checkDeployConfig(null);
    expect(result.name).toBe("deploy_config");
    expect(result.status).toBe("error");
    expect(result.message).toContain("agents.list[0].id: Required");
  });
});

describe("checkSecretRefs", () => {
  beforeEach(() => vi.resetModules());

  it("returns warn when anchor.json is absent", async () => {
    vi.doMock("../koina/fs.js", () => ({ readJson: vi.fn().mockReturnValue(null) }));
    const { checkSecretRefs } = await import("./diagnostics.js");
    const result = checkSecretRefs(null);
    expect(result.name).toBe("secret_refs");
    expect(result.status).toBe("warn");
  });

  it("returns ok with no SecretRef fields when config has inline strings", async () => {
    vi.doMock("../koina/fs.js", () => ({
      readJson: vi.fn()
        .mockReturnValueOnce({ deployDir: "/fake/deploy" })
        .mockReturnValueOnce({ models: { providers: { anthropic: { apiKey: "sk-abc" } } } }),
    }));
    const { checkSecretRefs } = await import("./diagnostics.js");
    const result = checkSecretRefs(null);
    expect(result.name).toBe("secret_refs");
    expect(result.status).toBe("ok");
    expect(result.message).toContain("No SecretRef fields detected");
  });

  it("returns ok with restart note when SecretRef fields are present", async () => {
    vi.doMock("../koina/fs.js", () => ({
      readJson: vi.fn()
        .mockReturnValueOnce({ deployDir: "/fake/deploy" })
        .mockReturnValueOnce({
          models: { providers: { anthropic: { apiKey: { source: "env", id: "ANTHROPIC_API_KEY" } } } },
        }),
    }));
    const { checkSecretRefs } = await import("./diagnostics.js");
    const result = checkSecretRefs(null);
    expect(result.name).toBe("secret_refs");
    expect(result.status).toBe("ok");
    expect(result.message).toContain("models.providers.anthropic.apiKey");
    expect(result.message).toContain("rotation requires a daemon restart");
  });

  it("returns warn when SecretRef has missing id field", async () => {
    vi.doMock("../koina/fs.js", () => ({
      readJson: vi.fn()
        .mockReturnValueOnce({ deployDir: "/fake/deploy" })
        .mockReturnValueOnce({
          gateway: { auth: { token: { source: "env" } } },
        }),
    }));
    const { checkSecretRefs } = await import("./diagnostics.js");
    const result = checkSecretRefs(null);
    expect(result.name).toBe("secret_refs");
    expect(result.status).toBe("warn");
    expect(result.message).toContain("missing source or id");
    expect(result.message).toContain("rotation requires a daemon restart");
  });
});
