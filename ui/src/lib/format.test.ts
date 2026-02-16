import { describe, it, expect, vi, afterEach } from "vitest";
import {
  formatTokens,
  formatUptime,
  formatTimeSince,
  formatCost,
  formatDuration,
  formatTimestamp,
} from "./format";

describe("formatTokens", () => {
  it("returns '0' for zero", () => {
    expect(formatTokens(0)).toBe("0");
  });

  it("returns raw number below 1k", () => {
    expect(formatTokens(500)).toBe("500");
  });

  it("returns rounded k for thousands", () => {
    expect(formatTokens(1500)).toBe("2k");
  });

  it("returns rounded k for large thousands", () => {
    expect(formatTokens(150_000)).toBe("150k");
  });

  it("returns M for millions", () => {
    expect(formatTokens(2_500_000)).toBe("2.5M");
  });
});

describe("formatUptime", () => {
  it("returns 0m for zero seconds", () => {
    expect(formatUptime(0)).toBe("0m");
  });

  it("returns minutes only when under an hour", () => {
    expect(formatUptime(120)).toBe("2m");
  });

  it("returns hours and minutes for large values", () => {
    expect(formatUptime(3700)).toBe("1h 1m");
  });
});

describe("formatTimeSince", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("returns 'never' for null", () => {
    expect(formatTimeSince(null)).toBe("never");
  });

  it("returns 'just now' for a date less than a minute ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-15T12:00:30Z"));
    expect(formatTimeSince("2026-01-15T12:00:00Z")).toBe("just now");
  });

  it("returns minutes ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-15T12:05:00Z"));
    expect(formatTimeSince("2026-01-15T12:00:00Z")).toBe("5m ago");
  });

  it("returns hours ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-15T15:00:00Z"));
    expect(formatTimeSince("2026-01-15T12:00:00Z")).toBe("3h ago");
  });

  it("returns days ago", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-17T12:00:00Z"));
    expect(formatTimeSince("2026-01-15T12:00:00Z")).toBe("2d ago");
  });
});

describe("formatCost", () => {
  it("formats zero", () => {
    expect(formatCost(0)).toBe("$0.0000");
  });

  it("formats to four decimal places", () => {
    expect(formatCost(0.1234)).toBe("$0.1234");
  });
});

describe("formatDuration", () => {
  it("returns ms for sub-second values", () => {
    expect(formatDuration(50)).toBe("50ms");
  });

  it("returns seconds for values >= 1000ms", () => {
    expect(formatDuration(1500)).toBe("1.5s");
  });
});

describe("formatTimestamp", () => {
  it("formats a valid ISO string to locale time", () => {
    const result = formatTimestamp("2026-01-15T14:30:00Z");
    // Locale-dependent, but should contain a colon and AM/PM
    expect(result).toMatch(/\d{1,2}:\d{2}\s?(AM|PM)/);
  });
});
