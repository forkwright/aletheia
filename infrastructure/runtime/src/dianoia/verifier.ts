// GoalBackwardVerifier — dispatches sub-agent to verify phase success criteria, persists result
import type Database from "better-sqlite3";
import { generateId } from "../koina/crypto.js";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningStore } from "./store.js";
import type { PlanningProject, VerificationGap, VerificationResult } from "./types.js";
import type { PhasePlan } from "./roadmap.js";
import { buildContextPacket } from "./context-packet.js";

const log = createLogger("dianoia:verifier");

interface VerifierAgentResult {
  status: VerificationResult["status"];
  summary: string;
  gaps: VerificationGap[];
}

interface DispatchOutput {
  results: Array<{
    index: number;
    status: "success" | "error" | "timeout";
    result?: string;
    error?: string;
    durationMs: number;
  }>;
}

export class GoalBackwardVerifier {
  private store: PlanningStore;
  private workspaceRoot: string | null = null;

  constructor(
    db: Database.Database,
    private dispatchTool: ToolHandler,
  ) {
    this.store = new PlanningStore(db);
  }

  /** Set workspace root for context packet assembly from file-backed state */
  setWorkspaceRoot(root: string): void {
    this.workspaceRoot = root;
  }

  async verify(
    projectId: string,
    phaseId: string,
    toolContext: ToolContext,
  ): Promise<VerificationResult> {
    const project = this.store.getProjectOrThrow(projectId);

    if (project.config.verifier === false) {
      const result: VerificationResult = {
        status: "met",
        summary: "Verification disabled.",
        gaps: [],
        verifiedAt: new Date().toISOString(),
      };
      this.store.updatePhaseVerificationResult(phaseId, result);
      return result;
    }

    const result = await this.runVerifierAgent(project, phaseId, toolContext);
    this.store.updatePhaseVerificationResult(phaseId, result);
    return result;
  }

  generateGapPlans(phaseId: string, gaps: VerificationGap[]): PhasePlan[] {
    if (gaps.length === 0) return [];

    return gaps.map((gap) => {
      const planId = generateId("vrfy");
      const stepId = generateId("step");
      const plan: PhasePlan & { id: string; name: string } = {
        id: planId,
        name: `Fix: ${gap.criterion ?? 'unknown'}`,
        steps: [
          {
            id: stepId,
            description: gap.criterion ?? "",
            subtasks: [gap.proposedFix ?? ""],
            dependsOn: [],
          },
        ],
        dependencies: [phaseId],
        acceptanceCriteria: [gap.proposedFix ?? ""],
      };
      return plan;
    });
  }

  private async runVerifierAgent(
    project: PlanningProject,
    phaseId: string,
    toolContext: ToolContext,
  ): Promise<VerificationResult> {
    const phase = this.store.getPhaseOrThrow(phaseId);
    const allPhases = this.store.listPhases(project.id);

    // Build rich context packet from file-backed state
    const contextPacket = this.workspaceRoot
      ? buildContextPacket({
          workspaceRoot: this.workspaceRoot,
          projectId: project.id,
          phaseId,
          role: "verifier",
          phase,
          allPhases,
          projectGoal: project.goal,
          requirements: this.store
            .listRequirements(project.id)
            .filter((r) => r.tier === "v1" && phase.requirements.includes(r.reqId)),
          maxTokens: 10000,
        })
      : [
          `Phase goal: ${phase.goal}`,
          "",
          `Success criteria:\n${phase.successCriteria.map((c, i) => `${i + 1}. ${c}`).join("\n")}`,
          "",
          `Project goal: ${project.goal}`,
        ].join("\n");

    const task = {
      role: "reviewer" as const,
      task: [
        "You are a goal-backward verifier. Evaluate whether this phase's success criteria are met.",
        "",
        contextPacket,
        "",
        "Report as JSON: { status: 'met'|'partially-met'|'not-met', summary: string, gaps: Array<{ criterion, status, detail, proposedFix }> }",
        "Each gap must include criterion (which criterion), status ('met'|'partially-met'|'not-met'), detail (evidence), and proposedFix (concrete next step).",
      ].join("\n"),
      timeoutSeconds: 120,
    };

    log.info(`Dispatching verifier sub-agent for phase ${phaseId}`);

    let raw: string;
    try {
      raw = await this.dispatchTool.execute({ tasks: [task] }, toolContext);
    } catch (err) {
      log.warn("Verifier dispatch failed — falling back to partially-met", { err, phaseId });
      return this.fallbackResult();
    }

    try {
      const dispatchOutput = JSON.parse(raw) as DispatchOutput;
      const firstResult = dispatchOutput.results[0];

      if (!firstResult || firstResult.status !== "success" || !firstResult.result) {
        return this.fallbackResult();
      }

      const parsed = JSON.parse(firstResult.result) as VerifierAgentResult;
      return {
        status: parsed.status,
        summary: parsed.summary,
        gaps: parsed.gaps ?? [],
        verifiedAt: new Date().toISOString(),
      };
    } catch (err) {
      log.warn("Verifier result parse error — falling back to partially-met", { err, phaseId });
      return this.fallbackResult();
    }
  }

  private fallbackResult(): VerificationResult {
    return {
      status: "partially-met",
      summary: "(verification unavailable)",
      gaps: [],
      verifiedAt: new Date().toISOString(),
    };
  }
}
