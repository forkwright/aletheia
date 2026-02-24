// ResearchOrchestrator — dispatches 4 parallel dimension researchers via sessions_dispatch
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import { writeResearchFile } from "./project-files.js";
import { z } from "zod";

const log = createLogger("dianoia:researcher");

// Zod schema for researcher response validation
const ResearcherResponseSchema = z.object({
  summary: z.string(),
  details: z.string(),
  confidence: z.enum(['high', 'medium', 'low']),
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
    private workspaceRoot?: string,
  ) {
    this.store = new PlanningStore(db);
  }

  private validateResearcherResponse(rawResult: string, dimension: string): { content: string; status: "complete" | "partial" } {
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
        // Store the validated structured response
        return { content: JSON.stringify(validated.data, null, 2), status: "complete" };
      } else {
        log.warn(`Validation failed for ${dimension} research response: ${validated.error.message}, storing raw text as partial`);
        return { content: rawResult, status: "partial" };
      }
    } catch (err) {
      log.warn(`Failed to parse ${dimension} research response: ${err instanceof Error ? err.message : String(err)}, storing raw text as partial`);
      return { content: rawResult, status: "partial" };
    }
  }

  async runResearch(
    projectId: string,
    projectGoal: string,
    toolContext: ToolContext,
    timeoutSeconds = 90,
  ): Promise<{ stored: number; partial: number; failed: number; synthesisText: string }> {
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

    // Fail-fast if all dimensions failed
    if (stored === 0) {
      throw new Error(`Research failed: No dimensions completed successfully (${failed} failed, ${partial} partial)`);
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

    const synthPrompt =
      `Produce a consolidated research summary for this project: "${projectGoal}"\n\n` +
      `Sections required: ## Stack, ## Features, ## Architecture, ## Pitfalls, ## Recommendations\n\n` +
      `Use the per-dimension findings below. Note any dimensions with partial or missing data.${partialNote}\n\n` +
      `Per-dimension findings:\n\n${dimensionSections || "(no completed dimensions)"}`;

    log.info(`Synthesizing research for project ${projectId} from ${completedRows.length} completed dimensions`);

    const raw = await this.dispatchTool.execute(
      {
        tasks: [
          {
            role: "researcher",
            task: synthPrompt,
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
