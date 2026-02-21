import { describe, expect, it } from "vitest";
import { truncateToolResult } from "./truncate.js";

describe("truncateToolResult", () => {
  it("returns short results unchanged", () => {
    expect(truncateToolResult("exec", "hello")).toBe("hello");
  });

  it("truncates exec results at 8K", () => {
    const long = "x".repeat(20000);
    const result = truncateToolResult("exec", long);
    expect(result.length).toBeLessThanOrEqual(8200); // limit + gap notice
    expect(result).toContain("chars omitted");
  });

  it("truncates read results at 10K", () => {
    const long = "x".repeat(20000);
    const result = truncateToolResult("read", long);
    expect(result.length).toBeLessThanOrEqual(10200);
    expect(result).toContain("chars omitted");
  });

  it("uses default 5K for unknown tools", () => {
    const long = "x".repeat(10000);
    const result = truncateToolResult("unknown_tool", long);
    expect(result.length).toBeLessThanOrEqual(5200);
    expect(result).toContain("chars omitted");
  });

  it("preserves head 70% and tail 30%", () => {
    const input = "HEAD".repeat(2000) + "TAIL".repeat(2000);
    const result = truncateToolResult("exec", input);
    expect(result.startsWith("HEAD")).toBe(true);
    expect(result.endsWith("TAIL")).toBe(true);
  });
});
