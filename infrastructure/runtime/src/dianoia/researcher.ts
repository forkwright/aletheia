// ResearchOrchestrator — dispatches 4 parallel dimension researchers via sessions_dispatch
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";

const log = createLogger("dianoia:researcher");

export const DIMENSIONS = ["stack", "features", "architecture", "pitfalls"] as const;
export type ResearchDimension = (typeof DIMENSIONS)[number];

const DIMENSION_SOULS: Record<ResearchDimension, string> = {
  stack:
    "You are a technology stack researcher. Analyze the technology landscape for this project: what languages, frameworks, databases, and libraries are standard choices. What are the tradeoffs between the top options? What does the community use in 2025/2026?",
  features:
    "You are a features researcher. Analyze the feature landscape for this domain: what capabilities are table-stakes (users expect them), what are differentiators (set products apart), and what are advanced/v2 features? Be specific and enumerate concrete features.",
  architecture:
    "You are an architecture researcher. Analyze architectural patterns for this domain: what system designs, data models, API patterns, and structural choices are standard? What are the scaling considerations and common design decisions?",
  pitfalls:
    "You are a pitfalls researcher. Identify the known failure modes, anti-patterns, gotchas, and common mistakes for this domain. What do developers typically get wrong? What technical debt accumulates? What security/performance traps exist?",
};

interface DispatchResult {
  index: number;
  status: "success" | "error" | "timeout";
  result?: string;
  error?: string;
  durationMs: number;
}

interface DispatchOutput {
  results: DispatchResult[];
}

export class ResearchOrchestrator {
  private store: PlanningStore;

  constructor(
    db: Database.Database,
    private dispatchTool: ToolHandler,
  ) {
    this.store = new PlanningStore(db);
  }

  async runResearch(
    projectId: string,
    projectGoal: string,
    toolContext: ToolContext,
    timeoutSeconds = 90,
  ): Promise<{ stored: number; partial: number; failed: number }> {
    const tasks = DIMENSIONS.map((dimension) => ({
      role: "researcher",
      task: `Research the ${dimension} dimension for this project. Return findings as a \`\`\`json block with fields: summary (2-3 sentences), details (full findings), confidence ('high'|'medium'|'low'). Project: ${projectGoal}`,
      context: DIMENSION_SOULS[dimension],
      timeoutSeconds,
    }));

    log.info(`Dispatching ${tasks.length} research tasks for project ${projectId}`);

    const raw = await this.dispatchTool.execute({ tasks }, toolContext);
    const dispatchOutput = JSON.parse(raw) as DispatchOutput;

    let stored = 0;
    let partial = 0;
    let failed = 0;

    for (let i = 0; i < DIMENSIONS.length; i++) {
      const dimension = DIMENSIONS[i]!;
      const result = dispatchOutput.results[i];

      if (!result) {
        this.store.createResearch({
          projectId,
          phase: "research",
          dimension,
          content: JSON.stringify({ reason: "error", error: "missing result" }),
          status: "failed",
        });
        failed++;
        continue;
      }

      if (result.status === "success") {
        this.store.createResearch({
          projectId,
          phase: "research",
          dimension,
          content: result.result ?? "",
          status: "complete",
        });
        stored++;
      } else if (result.status === "timeout") {
        this.store.createResearch({
          projectId,
          phase: "research",
          dimension,
          content: JSON.stringify({ reason: "timeout", durationMs: result.durationMs }),
          status: "partial",
        });
        partial++;
      } else {
        this.store.createResearch({
          projectId,
          phase: "research",
          dimension,
          content: JSON.stringify({ reason: "error", error: result.error ?? "" }),
          status: "failed",
        });
        failed++;
      }
    }

    log.info(`Research complete for ${projectId}: stored=${stored}, partial=${partial}, failed=${failed}`);
    return { stored, partial, failed };
  }
}
