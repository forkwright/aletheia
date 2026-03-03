// Stuck detection — prevents blind retries when a sub-agent fails with the same error pattern
import { createLogger } from "../koina/logger.js";

const log = createLogger("dianoia:stuck-detection");

export interface ErrorSignature {
  pattern: string;
  count: number;
  firstSeen: string;
  lastSeen: string;
}

export interface StuckCheckResult {
  isStuck: boolean;
  signature: ErrorSignature;
}

export class StuckDetector {
  private signatures = new Map<string, ErrorSignature>();

  recordFailure(planId: string, errorMessage: string): StuckCheckResult {
    const normalized = normalizeErrorPattern(errorMessage);
    const key = `${planId}:${normalized}`;
    const now = new Date().toISOString();

    const existing = this.signatures.get(key);
    if (existing) {
      existing.count += 1;
      existing.lastSeen = now;
      log.warn(`Stuck pattern for plan ${planId}: "${normalized}" seen ${existing.count} times`);
      return { isStuck: true, signature: existing };
    }

    const signature: ErrorSignature = {
      pattern: normalized,
      count: 1,
      firstSeen: now,
      lastSeen: now,
    };
    this.signatures.set(key, signature);
    return { isStuck: false, signature };
  }

  clear(planId: string): void {
    const prefix = `${planId}:`;
    for (const key of this.signatures.keys()) {
      if (key.startsWith(prefix)) {
        this.signatures.delete(key);
      }
    }
  }

  getSignatures(planId: string): ErrorSignature[] {
    const prefix = `${planId}:`;
    const result: ErrorSignature[] = [];
    for (const [key, sig] of this.signatures) {
      if (key.startsWith(prefix)) result.push(sig);
    }
    return result;
  }
}

export function normalizeErrorPattern(errorMessage: string): string {
  return errorMessage
    .toLowerCase()
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 200);
}
