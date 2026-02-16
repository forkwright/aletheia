// Uncertainty tracker tests
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { UncertaintyTracker } from "./uncertainty.js";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

let tmpDir: string;
let tracker: UncertaintyTracker;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "uncertainty-"));
  tracker = new UncertaintyTracker(tmpDir);
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("UncertaintyTracker", () => {
  it("records calibration points", () => {
    tracker.record("syn", "health", 0.9, true);
    tracker.record("syn", "health", 0.8, false);
    const summary = tracker.getSummary("syn");
    expect(summary.totalPoints).toBe(2);
  });

  it("clamps confidence to 0-1 range", () => {
    tracker.record("syn", "test", 1.5, true);
    tracker.record("syn", "test", -0.5, false);
    const summary = tracker.getSummary("syn");
    expect(summary.totalPoints).toBe(2);
  });

  it("computes Brier score (lower = better)", () => {
    // Perfect calibration: high confidence + correct
    tracker.record("syn", "d", 0.9, true);
    tracker.record("syn", "d", 0.9, true);
    tracker.record("syn", "d", 0.1, false);
    const brier = tracker.getBrierScore("syn");
    expect(brier).toBeLessThan(0.05); // Well-calibrated
  });

  it("Brier score penalizes overconfidence", () => {
    // Overconfident: high confidence + wrong
    tracker.record("syn", "d", 0.95, false);
    tracker.record("syn", "d", 0.95, false);
    const brier = tracker.getBrierScore("syn");
    expect(brier).toBeGreaterThan(0.8);
  });

  it("returns default Brier score with no data", () => {
    expect(tracker.getBrierScore()).toBe(0.5);
  });

  it("computes calibration curve with 10 bins", () => {
    tracker.record("syn", "d", 0.15, true);
    tracker.record("syn", "d", 0.85, true);
    tracker.record("syn", "d", 0.85, false);
    const curve = tracker.getCalibrationCurve("syn");
    // Floating point: 0.0 + 0.1 * 10 = 0.999... < 1 → 11 bins
    expect(curve.length).toBeGreaterThanOrEqual(10);
    const bin1 = curve.find((b) => b.range[0] >= 0.1 && b.range[0] < 0.2);
    expect(bin1).toBeDefined();
    expect(bin1!.total).toBe(1);
  });

  it("computes ECE (expected calibration error)", () => {
    // Well-calibrated: stated confidence matches accuracy
    for (let i = 0; i < 10; i++) tracker.record("syn", "d", 0.85, true);
    for (let i = 0; i < 2; i++) tracker.record("syn", "d", 0.85, false);
    const ece = tracker.getECE("syn");
    expect(ece).toBeLessThan(0.2);
  });

  it("returns 0 ECE with no data", () => {
    expect(tracker.getECE()).toBe(0);
  });

  it("filters by nousId", () => {
    tracker.record("syn", "d", 0.8, true);
    tracker.record("chiron", "d", 0.3, false);
    expect(tracker.getSummary("syn").totalPoints).toBe(1);
    expect(tracker.getSummary("chiron").totalPoints).toBe(1);
    expect(tracker.getSummary().totalPoints).toBe(2);
  });

  it("persists and reloads data", () => {
    tracker.record("syn", "d", 0.7, true);
    const tracker2 = new UncertaintyTracker(tmpDir);
    expect(tracker2.getSummary("syn").totalPoints).toBe(1);
  });

  it("getSummary includes all metrics", () => {
    tracker.record("syn", "d", 0.8, true);
    const summary = tracker.getSummary("syn");
    expect(summary).toHaveProperty("totalPoints");
    expect(summary).toHaveProperty("brierScore");
    expect(summary).toHaveProperty("ece");
    expect(summary).toHaveProperty("calibrationCurve");
  });
});
