// Tool loop detection — catches repetitive tool call patterns without
// penalizing productive long-running sessions.
//
// Strategy: Track recent (toolName, inputHash) pairs in a sliding window.
// If the same pair appears too often, that's a loop. Two severity levels:
//   1. WARN — inject a nudge into the conversation so the model can self-correct
//   2. HALT — hard stop after the model ignores the warning
//
// This replaces the blunt MAX_TOOL_LOOPS cap as the primary safeguard.
// MAX_TOOL_LOOPS remains as an absolute ceiling (raised to 100).

import { createHash } from "node:crypto";
import { createLogger } from "../koina/logger.js";

const log = createLogger("nous:loop-detector");

export interface LoopDetectorConfig {
  /** Size of the sliding window to track recent calls (default: 12) */
  windowSize?: number;
  /** How many times the same (tool, inputHash) can repeat in the window before warning (default: 3) */
  warnThreshold?: number;
  /** How many times the same (tool, inputHash) can repeat in the window before halting (default: 5) */
  haltThreshold?: number;
  /** How many consecutive error results trigger a halt (default: 4) */
  consecutiveErrorThreshold?: number;
}

export type LoopVerdict = "ok" | "warn" | "halt";

export interface LoopCheckResult {
  verdict: LoopVerdict;
  reason?: string;
  /** The offending tool name, if any */
  tool?: string;
  /** How many times the pattern repeated */
  repetitions?: number;
}

interface CallRecord {
  tool: string;
  inputHash: string;
  isError: boolean;
}

/** Stable hash of tool input for deduplication. Order-insensitive for objects. */
function hashInput(input: unknown): string {
  const normalized = JSON.stringify(input, Object.keys((input as Record<string, unknown>) ?? {}).sort());
  return createHash("sha256").update(normalized).digest("hex").slice(0, 16);
}

export class LoopDetector {
  private window: CallRecord[] = [];
  private warningIssued = false;
  private readonly windowSize: number;
  private readonly warnThreshold: number;
  private readonly haltThreshold: number;
  private readonly consecutiveErrorThreshold: number;

  constructor(config: LoopDetectorConfig = {}) {
    this.windowSize = config.windowSize ?? 12;
    this.warnThreshold = config.warnThreshold ?? 3;
    this.haltThreshold = config.haltThreshold ?? 5;
    this.consecutiveErrorThreshold = config.consecutiveErrorThreshold ?? 4;
  }

  /** Record a tool call and check for loop patterns. Call after each tool execution. */
  record(tool: string, input: unknown, isError: boolean): LoopCheckResult {
    const inputHash = hashInput(input);
    this.window.push({ tool, inputHash, isError });

    // Trim window to size
    while (this.window.length > this.windowSize) {
      this.window.shift();
    }

    // Check 1: Same (tool, input) repetition
    const key = `${tool}:${inputHash}`;
    const repetitions = this.window.filter(
      (r) => `${r.tool}:${r.inputHash}` === key,
    ).length;

    if (repetitions >= this.haltThreshold) {
      log.warn(`Loop detected (halt): ${tool} called ${repetitions}x with same input in last ${this.windowSize} calls`);
      return {
        verdict: "halt",
        reason: `Tool "${tool}" has been called ${repetitions} times with identical input — this appears to be an infinite loop.`,
        tool,
        repetitions,
      };
    }

    if (repetitions >= this.warnThreshold && !this.warningIssued) {
      this.warningIssued = true;
      log.info(`Loop warning: ${tool} called ${repetitions}x with same input in last ${this.windowSize} calls`);
      return {
        verdict: "warn",
        reason: `You've called "${tool}" ${repetitions} times with the same input. If you're stuck, try a different approach rather than repeating the same call.`,
        tool,
        repetitions,
      };
    }

    // Check 2: Consecutive errors (different inputs but all failing)
    const recentErrors = this.tailErrors();
    if (recentErrors >= this.consecutiveErrorThreshold) {
      const lastTool = this.window[this.window.length - 1]!.tool;
      if (!this.warningIssued) {
        this.warningIssued = true;
        log.info(`Error streak: ${recentErrors} consecutive tool errors`);
        return {
          verdict: "warn",
          reason: `The last ${recentErrors} tool calls all failed. Consider stopping to reassess your approach rather than continuing to retry.`,
          tool: lastTool,
          repetitions: recentErrors,
        };
      }
      if (recentErrors >= this.consecutiveErrorThreshold * 2) {
        log.warn(`Error streak halt: ${recentErrors} consecutive tool errors`);
        return {
          verdict: "halt",
          reason: `${recentErrors} consecutive tool failures — halting to prevent further errors.`,
          tool: lastTool,
          repetitions: recentErrors,
        };
      }
    }

    return { verdict: "ok" };
  }

  /** Count consecutive errors from the end of the window */
  private tailErrors(): number {
    let count = 0;
    for (let i = this.window.length - 1; i >= 0; i--) {
      if (this.window[i]!.isError) count++;
      else break;
    }
    return count;
  }

  /** Reset warning state (e.g., after model acknowledges and changes approach) */
  resetWarning(): void {
    this.warningIssued = false;
  }
}
