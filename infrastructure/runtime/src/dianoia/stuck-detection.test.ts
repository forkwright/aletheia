// Tests for stuck detection — error pattern tracking and retry prevention
import { describe, expect, it } from "vitest";
import { normalizeErrorPattern, StuckDetector } from "./stuck-detection.js";

describe("StuckDetector", () => {
  it("first failure is not stuck", () => {
    const detector = new StuckDetector();
    const result = detector.recordFailure("plan-1", "TypeError: Cannot read property 'foo'");

    expect(result.isStuck).toBe(false);
    expect(result.signature.count).toBe(1);
    expect(result.signature.pattern).toContain("typeerror");
  });

  it("same error twice is stuck", () => {
    const detector = new StuckDetector();
    detector.recordFailure("plan-1", "Connection refused to localhost:5432");
    const result = detector.recordFailure("plan-1", "Connection refused to localhost:5432");

    expect(result.isStuck).toBe(true);
    expect(result.signature.count).toBe(2);
  });

  it("different errors are not stuck", () => {
    const detector = new StuckDetector();
    detector.recordFailure("plan-1", "Connection refused to localhost:5432");
    const result = detector.recordFailure("plan-1", "Timeout waiting for response");

    expect(result.isStuck).toBe(false);
    expect(result.signature.count).toBe(1);
  });

  it("clear resets stuck state", () => {
    const detector = new StuckDetector();
    detector.recordFailure("plan-1", "Connection refused");
    detector.recordFailure("plan-1", "Connection refused");
    detector.clear("plan-1");

    const result = detector.recordFailure("plan-1", "Connection refused");
    expect(result.isStuck).toBe(false);
    expect(result.signature.count).toBe(1);
  });

  it("normalizes case, whitespace, and truncation", () => {
    const detector = new StuckDetector();
    detector.recordFailure("plan-1", "  ERROR:  Connection   REFUSED  ");
    const result = detector.recordFailure("plan-1", "error: connection refused");

    expect(result.isStuck).toBe(true);
  });

  it("different plans with same error do not interfere", () => {
    const detector = new StuckDetector();
    detector.recordFailure("plan-1", "Connection refused");
    const result = detector.recordFailure("plan-2", "Connection refused");

    expect(result.isStuck).toBe(false);
    expect(result.signature.count).toBe(1);
  });

  it("getSignatures returns accumulated data", () => {
    const detector = new StuckDetector();
    detector.recordFailure("plan-1", "Error A");
    detector.recordFailure("plan-1", "Error B");
    detector.recordFailure("plan-1", "Error A");

    const sigs = detector.getSignatures("plan-1");
    expect(sigs).toHaveLength(2);

    const sigA = sigs.find((s) => s.pattern.includes("error a"));
    const sigB = sigs.find((s) => s.pattern.includes("error b"));
    expect(sigA?.count).toBe(2);
    expect(sigB?.count).toBe(1);
  });
});

describe("normalizeErrorPattern", () => {
  it("lowercases input", () => {
    expect(normalizeErrorPattern("ERROR")).toBe("error");
  });

  it("collapses whitespace", () => {
    expect(normalizeErrorPattern("foo   bar\tbaz")).toBe("foo bar baz");
  });

  it("trims leading and trailing whitespace", () => {
    expect(normalizeErrorPattern("  hello  ")).toBe("hello");
  });

  it("truncates to 200 characters", () => {
    const long = "a".repeat(300);
    expect(normalizeErrorPattern(long)).toHaveLength(200);
  });
});
