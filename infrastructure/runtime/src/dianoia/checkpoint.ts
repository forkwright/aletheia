// CheckpointSystem — evaluates risk levels, persists audit records, fires planning:checkpoint events
import { eventBus } from "../koina/event-bus.js";
import type { PlanningStore } from "./store.js";
import type { PlanningConfig } from "./types.js";

export type TrueBlockerCategory =
  | "irreversible-data-deletion"
  | "auth-failure"
  | "state-corruption";

interface EvaluateOpts {
  projectId: string;
  type: string;
  riskLevel: "low" | "medium" | "high";
  trueBlockerCategory?: TrueBlockerCategory;
  question: string;
  context: Record<string, unknown>;
  nousId: string;
  sessionId: string;
}

export class CheckpointSystem {
  constructor(
    private store: PlanningStore,
    private config: PlanningConfig,
  ) {}

  async evaluate(opts: EvaluateOpts): Promise<"approved" | "blocked"> {
    const { projectId, type, question, context, riskLevel, trueBlockerCategory } = opts;

    // Branch 1: true blocker — bypasses YOLO mode, always blocks
    if (trueBlockerCategory !== undefined) {
      this.store.createCheckpoint({ projectId, type, question, context });
      return "blocked";
    }

    // Branch 2: low risk — auto-approve silently
    if (riskLevel === "low") {
      const checkpoint = this.store.createCheckpoint({ projectId, type, question, context });
      this.store.resolveCheckpoint(checkpoint.id, "approved", { autoApproved: true });
      eventBus.emit("planning:checkpoint", { ...opts, decision: "approved", autoApproved: true });
      return "approved";
    }

    // Branch 3: medium risk — notify (non-blocking)
    if (riskLevel === "medium") {
      const checkpoint = this.store.createCheckpoint({ projectId, type, question, context });
      this.store.resolveCheckpoint(checkpoint.id, "notified", { autoApproved: false });
      eventBus.emit("planning:checkpoint", { ...opts, decision: "notified", autoApproved: false });
      return "approved";
    }

    // Branch 4: high risk in YOLO mode — auto-approve
    if (this.config.mode === "yolo") {
      const checkpoint = this.store.createCheckpoint({ projectId, type, question, context });
      this.store.resolveCheckpoint(checkpoint.id, "approved", { autoApproved: true });
      eventBus.emit("planning:checkpoint", { ...opts, decision: "approved", autoApproved: true });
      return "approved";
    }

    // Branch 5: high risk in interactive mode — block, caller must approve via plan_verify
    this.store.createCheckpoint({ projectId, type, question, context });
    return "blocked";
  }
}
