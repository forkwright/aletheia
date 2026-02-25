// ResearchOrchestrator — dispatches 4 parallel dimension researchers via sessions_dispatch
//
// v2 changes:
//   - Context packets: each researcher receives scoped project context via buildContextPacketSync()
//   - Retry/fallback: failed dimensions retry once with exponential backoff before marking failed
//   - Skip completed: dimensions already stored as "complete" are skipped on re-run
//   - Synthesis context: synthesizer receives project file context, not just raw dimension output

import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import { writeResearchFile } from "./project-files.js";
import { buildContextPacketSync } from "./context-packet.js";
import { z } from "zod";

const log = createLogger("dianoia:researcher");

// Zod schema for researcher response validation
const ResearcherResponseSchema = z.object({
  summary: z.string(),
  details: z.string(),
  confidence: z.enum(["high", "medium", "low"]),
});

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

/** Delay helper for retry backoff */
function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

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
  /** Retry backoff delay in ms. Override in tests to 0. */
  retryDelayMs = 2000;

  constructor(
    db: Database.Database,
    private dispatchTool: ToolHandler,
    private workspaceRoot?: string,
  ) {
    this.store = new PlanningStore(db);
  }

  private validateResearcherResponse(
    rawResult: string,
    dimension: string,
  ): { content: string; status: "complete" | "partial" } {
    try {
      // Try to extract JSON from the response (look for ```json blocks)
      const jsonMatch = rawResult.match(/```json\s*([\s\S]*?)\s*```/);
      if (!jsonMatch) {
        log.warn(`No JSON block found in ${dimension} research response, storing raw text as partial`);
        return { content: rawResult, status: "partial" };
      }

      const jsonStr = jsonMatch[1]!.trim();
      const parsed = JSON.parse(jsonStr);
      const validated = ResearcherResponseSchema.safeParse(parsed);

      if (validated.success) {
        return { content: JSON.stringify(validated.data, null, 2), status: "complete" };
      } else {
        log.warn(
          `Validation failed for ${dimension} research response: ${validated.error.message}, storing raw text as partial`,
        );
        return { content: rawResult, status: "partial" };
      }
    } catch (err) {
      log.warn(
        `Failed to parse ${dimension} research response: ${err instanceof Error ? err.message : String(err)}, storing raw text as partial`,
      );
      return { content: rawResult, status: "partial" };
    }
  }

  /**
   * Build the task prompt for a single research dimension.
   * Includes scoped context packet from file-backed project state.
   */
  private buildResearchTask(
    dimension: ResearchDimension,
    projectId: string,
    projectGoal: string,
    timeoutSeconds: number,
  ): { role: string; task: string; timeoutSeconds: number } {
    // Build context packet from file-backed state if workspace is available
    let contextSection = "";
    if (this.workspaceRoot) {
      try {
        contextSection = buildContextPacketSync({
          workspaceRoot: this.workspaceRoot,
          projectId,
          phaseId: null,
          role: "researcher",
          projectGoal,
          maxTokens: 4000, // Keep small — researchers need room for their own findings
        });
      } catch (err) {
        log.warn(`Failed to build context packet for ${dimension}: ${err instanceof Error ? err.message : String(err)}`);
      }
    }

    const taskParts = [
      DIMENSION_SOULS[dimension],
      "",
      `# Project: ${projectGoal}`,
    ];

    if (contextSection) {
      taskParts.push("", "## Project Context", "", contextSection);
    }

    taskParts.push(
      "",
      "## Output Format",
      "",
      "Return findings as a ```json block with fields:",
      "- summary: 2-3 sentences",
      "- details: full findings (be thorough)",
      "- confidence: 'high' | 'medium' | 'low'",
    );

    return {
      role: "researcher",
      task: taskParts.join("\n"),
      timeoutSeconds,
    };
  }

  /**
   * Dispatch a single dimension with one retry on failure.
   * The batch dispatch already failed for this dimension, so this is the retry.
   * One attempt with 2s backoff. Returns the result or null.
   */
  private async dispatchWithRetry(
    task: { role: string; task: string; timeoutSeconds: number },
    dimension: ResearchDimension,
    toolContext: ToolContext,
  ): Promise<DispatchResult | null> {
    await delay(this.retryDelayMs); // Backoff before retry
    log.info(`Retrying ${dimension} research individually after batch failure`);

    try {
      const raw = await this.dispatchTool.execute({ tasks: [task] }, toolContext);
      const output = JSON.parse(raw) as DispatchOutput;
      return output.results[0] ?? null;
    } catch (err) {
      log.warn(
        `${dimension} research individual retry failed: ${err instanceof Error ? err.message : String(err)}`,
      );
      return null;
    }
  }

  async runResearch(
    projectId: string,
    projectGoal: string,
    toolContext: ToolContext,
    timeoutSeconds = 90,
  ): Promise<{ stored: number; partial: number; failed: number; synthesisText: string }> {
    // Skip dimensions already completed (idempotent re-run)
    const existingResearch = this.store.listResearch(projectId);
    const completedDimensions = new Set(
      existingResearch
        .filter((r) => r.status === "complete" && r.dimension !== "synthesis")
        .map((r) => r.dimension),
    );

    const dimensionsToResearch = DIMENSIONS.filter((d) => !completedDimensions.has(d));

    if (dimensionsToResearch.length === 0) {
      log.info(`All dimensions already complete for project ${projectId}, skipping to synthesis`);
      const synthesisText = await this.synthesizeResearch(projectId, projectGoal, toolContext);
      return {
        stored: completedDimensions.size,
        partial: 0,
        failed: 0,
        synthesisText,
      };
    }

    log.info(
      `Dispatching ${dimensionsToResearch.length} research tasks for project ${projectId} (${completedDimensions.size} already complete)`,
    );

    // Build tasks with context packets
    const tasks = dimensionsToResearch.map((dimension) =>
      this.buildResearchTask(dimension, projectId, projectGoal, timeoutSeconds),
    );

    // Dispatch all dimensions in parallel (first attempt)
    let dispatchOutput: DispatchOutput;
    try {
      const raw = await this.dispatchTool.execute({ tasks }, toolContext);
      dispatchOutput = JSON.parse(raw) as DispatchOutput;
    } catch (err) {
      // Complete dispatch failure — retry each dimension individually
      log.warn(`Batch dispatch failed, falling back to individual dispatch: ${err instanceof Error ? err.message : String(err)}`);
      dispatchOutput = { results: [] };
    }

    let stored = completedDimensions.size;
    let partial = 0;
    let failed = 0;

    // Process results and identify failures for retry
    const retryDimensions: Array<{ dimension: ResearchDimension; task: typeof tasks[0] }> = [];

    for (let i = 0; i < dimensionsToResearch.length; i++) {
      const dimension = dimensionsToResearch[i]!;
      const result = dispatchOutput.results[i];

      if (!result || result.status !== "success") {
        // Queue for individual retry
        retryDimensions.push({ dimension, task: tasks[i]! });
        continue;
      }

      // Validate and store successful result
      const validated = this.validateResearcherResponse(result.result ?? "", dimension);
      this.store.createResearch({
        projectId,
        phase: "research",
        dimension,
        content: validated.content,
        status: validated.status,
      });
      if (validated.status === "complete") {
        stored++;
      } else {
        partial++;
      }
    }

    // Retry failed dimensions individually with backoff
    for (const { dimension, task } of retryDimensions) {
      const retryResult = await this.dispatchWithRetry(task, dimension, toolContext);

      if (retryResult && retryResult.status === "success") {
        const validated = this.validateResearcherResponse(retryResult.result ?? "", dimension);
        this.store.createResearch({
          projectId,
          phase: "research",
          dimension,
          content: validated.content,
          status: validated.status,
        });
        if (validated.status === "complete") {
          stored++;
        } else {
          partial++;
        }
      } else if (retryResult && retryResult.status === "timeout") {
        this.store.createResearch({
          projectId,
          phase: "research",
          dimension,
          content: JSON.stringify({ reason: "timeout", durationMs: retryResult.durationMs }),
          status: "partial",
        });
        partial++;
      } else {
        this.store.createResearch({
          projectId,
          phase: "research",
          dimension,
          content: JSON.stringify({
            reason: "error",
            error: retryResult?.error ?? "dispatch failed after retry",
          }),
          status: "failed",
        });
        failed++;
      }
    }

    log.info(`Research complete for ${projectId}: stored=${stored}, partial=${partial}, failed=${failed}`);

    // Fail-fast only if NO dimensions have usable content
    const usableCount = stored + partial;
    if (usableCount === 0) {
      throw new Error(
        `Research failed: No dimensions completed successfully (${failed} failed, ${partial} partial)`,
      );
    }

    const synthesisText = await this.synthesizeResearch(projectId, projectGoal, toolContext);
    return { stored, partial, failed, synthesisText };
  }

  async synthesizeResearch(
    projectId: string,
    projectGoal: string,
    toolContext: ToolContext,
  ): Promise<string> {
    const rows = this.store.listResearch(projectId).filter((r) => r.dimension !== "synthesis");

    const completedRows = rows.filter((r) => r.status === "complete");
    const partialRows = rows.filter((r) => r.status !== "complete");

    const dimensionSections = completedRows
      .map((r) => {
        const truncated = r.content.slice(0, 1500);
        return `### ${r.dimension}\n${truncated}`;
      })
      .join("\n\n");

    const partialNote =
      partialRows.length > 0
        ? `\n\nNote: The following dimensions have partial or missing data: ${partialRows.map((r) => r.dimension).join(", ")}.`
        : "";

    // Build context packet for synthesis — gives the synthesizer project awareness
    let contextSection = "";
    if (this.workspaceRoot) {
      try {
        contextSection = buildContextPacketSync({
          workspaceRoot: this.workspaceRoot,
          projectId,
          phaseId: null,
          role: "researcher",
          projectGoal,
          maxTokens: 3000,
        });
      } catch (err) {
        log.warn(`Failed to build context packet for synthesis: ${err instanceof Error ? err.message : String(err)}`);
      }
    }

    const synthParts = [
      `Produce a consolidated research summary for this project: "${projectGoal}"`,
    ];

    if (contextSection) {
      synthParts.push("", "## Project Context", "", contextSection);
    }

    synthParts.push(
      "",
      "## Output Requirements",
      "",
      "Sections required: ## Stack, ## Features, ## Architecture, ## Pitfalls, ## Recommendations",
      "",
      `Use the per-dimension findings below. Note any dimensions with partial or missing data.${partialNote}`,
      "",
      "## Per-Dimension Findings",
      "",
      dimensionSections || "(no completed dimensions)",
    );

    log.info(
      `Synthesizing research for project ${projectId} from ${completedRows.length} completed dimensions`,
    );

    const raw = await this.dispatchTool.execute(
      {
        tasks: [
          {
            role: "researcher",
            task: synthParts.join("\n"),
            timeoutSeconds: 120,
          },
        ],
      },
      toolContext,
    );

    const dispatchOutput = JSON.parse(raw) as DispatchOutput;
    const synthResult = dispatchOutput.results[0];
    const synthesisText =
      synthResult?.status === "success" ? (synthResult.result ?? "") : "(synthesis unavailable)";

    this.store.createResearch({
      projectId,
      phase: "research",
      dimension: "synthesis",
      content: synthesisText,
      status: "complete",
    });

    log.info(`Synthesis stored for project ${projectId}`);
    return synthesisText;
  }

  transitionToRequirements(projectId: string): void {
    // Write RESEARCH.md to disk
    if (this.workspaceRoot) {
      const research = this.store.listResearch(projectId);
      writeResearchFile(this.workspaceRoot, projectId, research);
    }

    this.store.updateProjectState(projectId, transition("researching", "RESEARCH_COMPLETE"));
  }
}
