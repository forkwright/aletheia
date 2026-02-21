// Tool result truncation tests
import { describe, expect, it } from "vitest";
import { truncateToolResult } from "./truncate.js";

describe("truncateToolResult", () => {
  it("returns short text unchanged", () => {
    const text = "hello world";
    expect(truncateToolResult(text)).toBe(text);
  });

  it("truncates long text with head/tail", () => {
    const text = "x".repeat(100_000);
    const result = truncateToolResult(text, { maxTokens: 100 });
    expect(result).toContain("chars omitted");
    expect(result.length).toBeLessThan(text.length);
  });

  it("uses custom headRatio for text format", () => {
    const text = "x".repeat(100_000);
    const result = truncateToolResult(text, { maxTokens: 100, headRatio: 0.5, format: "text" });
    expect(result).toContain("chars omitted");
  });

  it("detects and truncates JSON arrays", () => {
    const arr = Array.from({ length: 200 }, (_, i) => ({ id: i, name: `item_${i}` }));
    const text = JSON.stringify(arr, null, 2);
    const result = truncateToolResult(text, { maxTokens: 500 });
    expect(result).toContain("items omitted");
  });

  it("detects and truncates JSON objects", () => {
    const obj: Record<string, string> = {};
    for (let i = 0; i < 500; i++) obj[`key_${i}`] = "x".repeat(100);
    const text = JSON.stringify(obj, null, 2);
    const result = truncateToolResult(text, { maxTokens: 500 });
    expect(result).toContain("truncated");
  });

  it("detects and truncates line-based output", () => {
    const lines = Array.from({ length: 500 }, (_, i) => `line ${i}: ${"data".repeat(20)}`);
    const text = lines.join("\n");
    const result = truncateToolResult(text, { maxTokens: 500 });
    expect(result).toContain("lines omitted");
  });

  it("respects explicit format override", () => {
    const text = '{"key": "value"}\n'.repeat(100);
    const result = truncateToolResult(text, { maxTokens: 50, format: "lines" });
    expect(result).toContain("lines omitted");
  });

  it("falls back to text truncation for invalid JSON", () => {
    const text = "{not json" + "x".repeat(100_000);
    const result = truncateToolResult(text, { maxTokens: 100 });
    expect(result).toContain("chars omitted");
  });

  it("keeps small JSON arrays intact", () => {
    const arr = [1, 2, 3];
    const text = JSON.stringify(arr, null, 2);
    const result = truncateToolResult(text, { maxTokens: 500 });
    expect(result).toBe(text);
  });

  it("default maxTokens is 8000", () => {
    // 8000 tokens × 3.5 chars ≈ 28000 chars
    const shortEnough = "x".repeat(27000);
    expect(truncateToolResult(shortEnough)).toBe(shortEnough);

    const tooLong = "x".repeat(30000);
    expect(truncateToolResult(tooLong)).not.toBe(tooLong);
  });
});
