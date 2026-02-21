// Diagnostics tests
import { describe, expect, it, vi } from "vitest";
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
        fix: { description: "Do the thing", action: () => {} },
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
