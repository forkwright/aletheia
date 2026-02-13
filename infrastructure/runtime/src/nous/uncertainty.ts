// Uncertainty quantification — track calibration of agent confidence estimates
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

interface CalibrationPoint {
  nousId: string;
  domain: string;
  statedConfidence: number;  // 0.0-1.0 what the agent said
  wasCorrect: boolean;       // verified outcome
  timestamp: string;
}

interface CalibrationBin {
  range: [number, number];
  total: number;
  correct: number;
  accuracy: number;
}

export class UncertaintyTracker {
  private points: CalibrationPoint[] = [];
  private filePath: string;

  constructor(sharedRoot: string) {
    const dir = join(sharedRoot, "shared", "calibration");
    mkdirSync(dir, { recursive: true });
    this.filePath = join(dir, "points.json");
    this.load();
  }

  private load(): void {
    if (existsSync(this.filePath)) {
      try {
        this.points = JSON.parse(readFileSync(this.filePath, "utf-8"));
      } catch {
        this.points = [];
      }
    }
  }

  private save(): void {
    // Keep last 1000 points
    if (this.points.length > 1000) {
      this.points = this.points.slice(-1000);
    }
    writeFileSync(this.filePath, JSON.stringify(this.points, null, 2));
  }

  record(
    nousId: string,
    domain: string,
    statedConfidence: number,
    wasCorrect: boolean,
  ): void {
    this.points.push({
      nousId,
      domain,
      statedConfidence: Math.max(0, Math.min(1, statedConfidence)),
      wasCorrect,
      timestamp: new Date().toISOString(),
    });
    this.save();
  }

  // Get calibration curve — how well do stated confidences match reality
  getCalibrationCurve(nousId?: string): CalibrationBin[] {
    const relevant = nousId
      ? this.points.filter((p) => p.nousId === nousId)
      : this.points;

    const bins: CalibrationBin[] = [];
    const binSize = 0.1;

    for (let low = 0; low < 1; low += binSize) {
      const high = low + binSize;
      const inBin = relevant.filter(
        (p) => p.statedConfidence >= low && p.statedConfidence < high,
      );
      const correct = inBin.filter((p) => p.wasCorrect).length;

      bins.push({
        range: [Math.round(low * 100) / 100, Math.round(high * 100) / 100],
        total: inBin.length,
        correct,
        accuracy: inBin.length > 0 ? correct / inBin.length : 0,
      });
    }

    return bins;
  }

  // Brier score — lower is better calibrated (0 = perfect, 1 = worst)
  getBrierScore(nousId?: string): number {
    const relevant = nousId
      ? this.points.filter((p) => p.nousId === nousId)
      : this.points;

    if (relevant.length === 0) return 0.5;

    const sum = relevant.reduce((acc, p) => {
      const outcome = p.wasCorrect ? 1 : 0;
      return acc + (p.statedConfidence - outcome) ** 2;
    }, 0);

    return sum / relevant.length;
  }

  // Expected Calibration Error — difference between stated and actual accuracy
  getECE(nousId?: string): number {
    const curve = this.getCalibrationCurve(nousId);
    let ece = 0;
    let totalPoints = 0;

    for (const bin of curve) {
      if (bin.total === 0) continue;
      const midpoint = (bin.range[0] + bin.range[1]) / 2;
      ece += bin.total * Math.abs(bin.accuracy - midpoint);
      totalPoints += bin.total;
    }

    return totalPoints > 0 ? ece / totalPoints : 0;
  }

  getSummary(nousId?: string): Record<string, unknown> {
    const relevant = nousId
      ? this.points.filter((p) => p.nousId === nousId)
      : this.points;

    return {
      totalPoints: relevant.length,
      brierScore: Math.round(this.getBrierScore(nousId) * 1000) / 1000,
      ece: Math.round(this.getECE(nousId) * 1000) / 1000,
      calibrationCurve: this.getCalibrationCurve(nousId),
    };
  }
}
