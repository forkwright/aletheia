import { describe, it, expect } from "vitest";
import { LoopDetector } from "./loop-detector.js";

describe("LoopDetector", () => {
  it("returns ok for diverse tool calls", () => {
    const detector = new LoopDetector();
    for (let i = 0; i < 20; i++) {
      const result = detector.record("exec", { command: `cmd_${i}` }, false);
      expect(result.verdict).toBe("ok");
    }
  });

  it("warns on repeated same-input calls", () => {
    const detector = new LoopDetector({ warnThreshold: 3 });
    const input = { command: "ls /tmp" };
    detector.record("exec", input, false);
    detector.record("exec", input, false);
    const result = detector.record("exec", input, false);
    expect(result.verdict).toBe("warn");
    expect(result.tool).toBe("exec");
    expect(result.repetitions).toBe(3);
    expect(result.reason).toContain("same input");
  });

  it("halts after exceeding halt threshold", () => {
    const detector = new LoopDetector({ warnThreshold: 2, haltThreshold: 4 });
    const input = { path: "/same/file" };
    detector.record("read", input, false); // 1
    detector.record("read", input, false); // 2 → warn
    detector.record("read", input, false); // 3 → already warned, ok
    const result = detector.record("read", input, false); // 4 → halt
    expect(result.verdict).toBe("halt");
    expect(result.repetitions).toBe(4);
  });

  it("only warns once per escalation", () => {
    const detector = new LoopDetector({ warnThreshold: 2, haltThreshold: 6 });
    const input = { x: 1 };
    detector.record("exec", input, false); // 1
    const warn = detector.record("exec", input, false); // 2 → warn
    expect(warn.verdict).toBe("warn");

    const next = detector.record("exec", input, false); // 3 → already warned, ok (not at halt yet)
    expect(next.verdict).toBe("ok");
  });

  it("does not trigger for same tool with different inputs", () => {
    const detector = new LoopDetector({ warnThreshold: 2 });
    for (let i = 0; i < 12; i++) {
      const result = detector.record("exec", { command: `different_${i}` }, false);
      expect(result.verdict).toBe("ok");
    }
  });

  it("sliding window forgets old calls", () => {
    const detector = new LoopDetector({ windowSize: 4, warnThreshold: 3 });
    const target = { cmd: "repeat" };
    detector.record("exec", target, false); // 1
    detector.record("exec", target, false); // 2
    // Push 3 different calls to age out the repeats
    detector.record("exec", { cmd: "a" }, false);
    detector.record("exec", { cmd: "b" }, false);
    detector.record("exec", { cmd: "c" }, false);
    // Now the target calls have fallen out of the window
    const result = detector.record("exec", target, false); // only 1 in window
    expect(result.verdict).toBe("ok");
  });

  it("detects consecutive error streaks", () => {
    const detector = new LoopDetector({ consecutiveErrorThreshold: 3 });
    detector.record("exec", { cmd: "a" }, true);
    detector.record("exec", { cmd: "b" }, true);
    const result = detector.record("exec", { cmd: "c" }, true);
    expect(result.verdict).toBe("warn");
    expect(result.reason).toContain("failed");
  });

  it("resets error streak on success", () => {
    const detector = new LoopDetector({ consecutiveErrorThreshold: 3 });
    detector.record("exec", { cmd: "a" }, true);
    detector.record("exec", { cmd: "b" }, true);
    detector.record("exec", { cmd: "c" }, false); // success breaks streak
    const result = detector.record("exec", { cmd: "d" }, true); // only 1 error
    expect(result.verdict).toBe("ok");
  });

  it("halts on extended error streak", () => {
    const detector = new LoopDetector({ consecutiveErrorThreshold: 2 });
    // Fill window with errors — need 2 to warn, 4 to halt
    detector.record("exec", { cmd: "a" }, true);
    detector.record("exec", { cmd: "b" }, true); // warn at 2
    detector.record("exec", { cmd: "c" }, true);
    const result = detector.record("exec", { cmd: "d" }, true); // halt at 4 (2*2)
    expect(result.verdict).toBe("halt");
  });

  it("resetWarning allows re-warning", () => {
    const detector = new LoopDetector({ warnThreshold: 2, haltThreshold: 10 });
    const input = { x: 1 };
    detector.record("exec", input, false);
    const first = detector.record("exec", input, false);
    expect(first.verdict).toBe("warn");

    detector.resetWarning();
    // Still has repetitions in window, so next check at threshold triggers again
    const second = detector.record("exec", input, false);
    expect(second.verdict).toBe("warn");
  });

  it("handles different tools with same input independently", () => {
    const detector = new LoopDetector({ warnThreshold: 3 });
    const input = { path: "/same" };
    detector.record("read", input, false);
    detector.record("write", input, false);
    detector.record("read", input, false);
    detector.record("write", input, false);
    const result = detector.record("read", input, false); // read: 3rd time
    expect(result.verdict).toBe("warn");
    expect(result.tool).toBe("read");
  });

  it("uses stable hash regardless of key order", () => {
    const detector = new LoopDetector({ warnThreshold: 2 });
    detector.record("exec", { a: 1, b: 2 }, false);
    const result = detector.record("exec", { b: 2, a: 1 }, false);
    // Same keys+values, different order — should be treated as same input
    expect(result.verdict).toBe("warn");
  });
});
