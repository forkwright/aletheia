// Sanitize tests
import { describe, expect, it } from "vitest";
import { sanitizeError, sanitizeForLog } from "./sanitize.js";

describe("sanitizeForLog", () => {
  it("returns short text unchanged", () => {
    expect(sanitizeForLog("hello")).toBe("hello");
  });

  it("returns text at maxLen unchanged", () => {
    const text = "a".repeat(200);
    expect(sanitizeForLog(text)).toBe(text);
  });

  it("truncates long text with redaction", () => {
    const text = "x".repeat(300);
    const result = sanitizeForLog(text);
    expect(result).toContain("...[redacted]...");
    expect(result.startsWith("x".repeat(50))).toBe(true);
    expect(result.endsWith("x".repeat(20))).toBe(true);
    expect(result.length).toBeLessThan(300);
  });

  it("returns empty string for empty input", () => {
    expect(sanitizeForLog("")).toBe("");
  });

  it("respects custom maxLen", () => {
    const text = "a".repeat(100);
    expect(sanitizeForLog(text, 50)).toContain("...[redacted]...");
    expect(sanitizeForLog(text, 200)).toBe(text);
  });
});

describe("sanitizeError", () => {
  it("strips long quoted content", () => {
    const longQuote = `"${"a".repeat(250)}"`;
    const msg = `Error processing: ${longQuote} in handler`;
    expect(sanitizeError(msg)).toContain('"[content redacted]"');
    expect(sanitizeError(msg)).not.toContain("a".repeat(250));
  });

  it("preserves short quoted content", () => {
    const msg = 'Error: "short quote" in handler';
    expect(sanitizeError(msg)).toBe(msg);
  });

  it("handles message with no quotes", () => {
    expect(sanitizeError("plain error message")).toBe("plain error message");
  });
});
