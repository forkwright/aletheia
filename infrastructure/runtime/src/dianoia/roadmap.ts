// RoadmapOrchestrator — generates phased roadmap, validates coverage, plans each phase with checker loop
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import { PlanningError } from "../koina/errors.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import type { PlanningPhase } from "./types.js";
import { buildContextPacketSync } from "./context-packet.js";

const log = createLogger("dianoia:roadmap");

const MAX_ITERATIONS = 3;

export interface PhaseDefinition {
  name: string;
  goal: string;
  requirements: string[];
  successCriteria: string[];
  phaseOrder: number;
  /** Phase names this phase depends on. Resolved to IDs during commitRoadmap(). */
  dependsOn?: string[];
}

export interface PlanStep {
  id: string;
  description: string;
  subtasks: string[];
  dependsOn: string[];
}

export interface PhasePlan {
  steps: PlanStep[];
  dependencies: string[];
  acceptanceCriteria: string[];
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

export class RoadmapOrchestrator {
  private store: PlanningStore;
  private workspaceRoot: string | null = null;

  constructor(
    private db: Database.Database,
    private dispatchTool: ToolHandler,
  ) {
    this.store = new PlanningStore(db);
  }

  /** Set workspace root for context packet assembly from file-backed state */
  setWorkspaceRoot(root: string): void {
    this.workspaceRoot = root;
  }

  async generateRoadmap(
    projectId: string,
    projectGoal: string,
    toolContext: ToolContext,
  ): Promise<PhaseDefinition[]> {
    const v1Reqs = this.store
      .listRequirements(projectId)
      .filter((r) => r.tier === "v1");

    // Build context packet from file-backed state for planner awareness
    let contextSection = "";
    if (this.workspaceRoot) {
      try {
        contextSection = buildContextPacketSync({
          workspaceRoot: this.workspaceRoot,
          projectId,
          phaseId: null,
          role: "planner",
          projectGoal,
          requirements: v1Reqs,
          maxTokens: 8000,
        });
      } catch (error) {
        log.warn(`Failed to build context packet for roadmap generation: ${error instanceof Error ? error.message : String(error)}`);
      }
    }

    const taskParts = [
      `Generate a phased roadmap for this project: "${projectGoal}"`,
      "",
      "Rules:",
      "- Group requirements by category code (e.g., AUTH, API, STOR) — same category = same phase",
      "- Foundation-first ordering: core auth/data before API, API before UI",
      "- Orphaned requirements that don't fit naturally go into a catch-all phase",
      "- No target phase count — let requirements drive it",
    ];

    if (contextSection) {
      taskParts.push("", "## Project Context", "", contextSection);
    } else {
      // Fallback: inline requirements as JSON when no file-backed state
      taskParts.push("", `v1 requirements (must all be covered):\n${JSON.stringify(v1Reqs, null, 2)}`);
    }

    taskParts.push(
      "",
      "Return a ```json block with a PhaseDefinition[] array. Each item: { name, goal, requirements (REQ-ID strings), successCriteria (2-5 observable strings), phaseOrder (1-based integer), dependsOn (array of phase NAMES this phase depends on, empty [] if none) }",
    );

    const task = {
      role: "planner",
      task: taskParts.join("\n"),
      context:
        "You are a software project planner. Create a phased roadmap that groups requirements logically and orders phases by dependency.",
      timeoutSeconds: 120,
    };

    log.info(`Dispatching roadmap generation for project ${projectId}`);

    const raw = await this.dispatchTool.execute({ tasks: [task] }, toolContext);
    const dispatchOutput = JSON.parse(raw) as DispatchOutput;
    const result = dispatchOutput.results[0];

    if (!result || result.status !== "success" || !result.result) {
      throw new PlanningError("Roadmap generation agent failed", {
        code: "PLANNING_STATE_CORRUPT",
        context: { projectId, status: result?.status },
      });
    }

    const jsonMatch = /```json\s*([\s\S]*?)```/.exec(result.result);
    if (!jsonMatch?.[1]) {
      throw new PlanningError("Roadmap agent returned no JSON block", {
        code: "PLANNING_STATE_CORRUPT",
        context: { projectId },
      });
    }

    const phases = JSON.parse(jsonMatch[1]) as PhaseDefinition[];
    log.info(`Roadmap generated: ${phases.length} phases for project ${projectId}`);
    return phases;
  }

  commitRoadmap(projectId: string, phases: PhaseDefinition[]): void {
    const commit = this.db.transaction(() => {
      this.db.prepare("DELETE FROM planning_phases WHERE project_id = ?").run(projectId);

      // First pass: create phases without dependencies (need IDs first)
      const nameToId = new Map<string, string>();
      const createdPhases: Array<{ phase: PhaseDefinition; id: string }> = [];

      for (const phase of phases) {
        const created = this.store.createPhase({
          projectId,
          name: phase.name,
          goal: phase.goal,
          requirements: phase.requirements ?? [],
          successCriteria: phase.successCriteria ?? [],
          phaseOrder: phase.phaseOrder,
        });
        nameToId.set(phase.name, created.id);
        createdPhases.push({ phase, id: created.id });
      }

      // Second pass: resolve name-based dependencies to phase IDs
      for (const { phase, id } of createdPhases) {
        if (phase.dependsOn && phase.dependsOn.length > 0) {
          const resolvedDeps = phase.dependsOn
            .map((depName) => nameToId.get(depName))
            .filter((depId): depId is string => depId !== undefined);
          if (resolvedDeps.length > 0) {
            this.store.updatePhaseDependencies(id, resolvedDeps);
          }
        }
      }
    });
    commit();
    log.info(`Committed ${phases.length} phases for project ${projectId}`);
  }

  validateCoverage(
    projectId: string,
    phases: PhaseDefinition[],
  ): { covered: boolean; missing: string[] } {
    const v1ReqIds = this.store
      .listRequirements(projectId)
      .filter((r) => r.tier === "v1")
      .map((r) => r.reqId);

    const coveredSet = new Set(phases.flatMap((p) => p.requirements));
    const missing = v1ReqIds.filter((id) => !coveredSet.has(id));
    return { covered: missing.length === 0, missing };
  }

  validateCoverageFromDb(projectId: string): { covered: boolean; missing: string[] } {
    const phases = this.store.listPhases(projectId);
    const v1ReqIds = this.store
      .listRequirements(projectId)
      .filter((r) => r.tier === "v1")
      .map((r) => r.reqId);

    const coveredSet = new Set(phases.flatMap((p) => p.requirements));
    const missing = v1ReqIds.filter((id) => !coveredSet.has(id));
    return { covered: missing.length === 0, missing };
  }

  listPhases(projectId: string): PlanningPhase[] {
    return this.store.listPhases(projectId);
  }

  adjustPhase(
    projectId: string,
    adjustment: string,
    opts: {
      phaseName?: string;
      requirements?: string[];
      newName?: string;
      newGoal?: string;
    },
  ): void {
    log.debug(`adjustPhase: ${adjustment}`, { projectId, opts });

    const phases = this.store.listPhases(projectId);
    const phase = phases.find((p) => p.name === opts.phaseName);
    if (!phase) {
      throw new PlanningError(`Phase not found: ${opts.phaseName ?? "(undefined)"}`, {
        code: "PLANNING_PHASE_NOT_FOUND",
        context: { projectId, phaseName: opts.phaseName },
      });
    }

    const sets: string[] = [];
    const vals: unknown[] = [];

    if (opts.newName !== undefined) {
      sets.push("name = ?");
      vals.push(opts.newName);
    }
    if (opts.newGoal !== undefined) {
      sets.push("goal = ?");
      vals.push(opts.newGoal);
    }
    if (opts.requirements !== undefined) {
      sets.push("requirements = ?");
      vals.push(JSON.stringify(opts.requirements));
    }

    if (sets.length === 0) return;

    sets.push("updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')");
    vals.push(phase.id);

    this.db
      .prepare(`UPDATE planning_phases SET ${sets.join(", ")} WHERE id = ?`)
      .run(...(vals as Parameters<ReturnType<Database.Database["prepare"]>["run"]>));
  }

  async planPhase(
    projectId: string,
    phaseId: string,
    config: { plan_check?: boolean },
    toolContext: ToolContext,
  ): Promise<PhasePlan> {
    const phase = this.store.getPhaseOrThrow(phaseId);
    const project = this.store.getProjectOrThrow(projectId);
    const depth = project.config.depth ?? "standard";
    const depthInstruction = this.depthToInstruction(depth);

    let plan = await this.generatePlanForPhase(phase, depthInstruction, toolContext, projectId, project.goal);

    if (config.plan_check === true) {
      for (let attempt = 1; attempt <= MAX_ITERATIONS; attempt++) {
        const check = await this.checkPlan(phase, plan, toolContext, attempt, projectId, project.goal);
        if (check.pass) break;
        if (attempt < MAX_ITERATIONS) {
          plan = await this.revisePlan(phase, plan, check.issues, depthInstruction, toolContext, projectId, project.goal);
        } else {
          log.warn(`Plan checker failed ${MAX_ITERATIONS} times for phase ${phaseId} — using best-effort plan`, {
            issues: check.issues,
          });
        }
      }
    }

    this.store.updatePhasePlan(phaseId, plan);
    return plan;
  }

  depthToInstruction(depth: string): string {
    switch (depth) {
      case "quick":
        return "Produce a brief plan: 1-3 high-level steps, minimal subtasks.";
      case "comprehensive":
        return "Produce a detailed plan: 5-10 steps with subtasks, explicit dependencies, full acceptance criteria.";
      default:
        return "Produce a standard plan: 3-5 steps with key subtasks and acceptance criteria.";
    }
  }

  formatRoadmapDisplay(phases: Array<PhaseDefinition | PlanningPhase>): string {
    const lines: string[] = ["## Generated Roadmap", ""];

    for (const phase of phases) {
      const order = "phaseOrder" in phase ? phase.phaseOrder : (phase as PlanningPhase).phaseOrder;
      lines.push(`### Phase ${order}: ${phase.name}`);
      lines.push(`**Goal:** ${phase.goal}`);

      const reqs = phase.requirements;
      if (reqs.length > 0) {
        lines.push(`**Requirements:** ${reqs.join(", ")}`);
      } else {
        lines.push(`**Requirements:** (none)`);
      }

      const criteria = phase.successCriteria;
      if (criteria.length > 0) {
        lines.push("**Success criteria:**");
        for (const c of criteria) {
          lines.push(`- ${c}`);
        }
      }

      lines.push("");
    }

    lines.push(
      "Adjust anything? (e.g., 'Move AUTH-01 to Phase 1', 'Rename Phase 3 to Integration', 'done' to commit)",
    );

    return lines.join("\n");
  }

  transitionToPhysicalPlanning(projectId: string): void {
    this.store.updateProjectState(projectId, transition("roadmap", "ROADMAP_COMPLETE"));
  }

  transitionToExecution(projectId: string): void {
    this.store.updateProjectState(projectId, transition("phase-planning", "PLAN_READY"));
  }

  private async generatePlanForPhase(
    phase: PlanningPhase,
    depthInstruction: string,
    toolContext: ToolContext,
    projectId?: string,
    projectGoal?: string,
  ): Promise<PhasePlan> {
    // Build context packet from file-backed state if available
    let contextSection = "";
    if (this.workspaceRoot && projectId) {
      const allPhases = this.store.listPhases(projectId);
      contextSection = buildContextPacketSync({
        workspaceRoot: this.workspaceRoot,
        projectId,
        phaseId: phase.id,
        role: "planner",
        phase,
        allPhases,
        projectGoal: projectGoal ?? "",
        requirements: this.store
          .listRequirements(projectId)
          .filter((r) => r.tier === "v1" && phase.requirements.includes(r.reqId)),
        maxTokens: 10000,
      });
    }

    const task = {
      role: "planner",
      task: [
        `Generate an implementation plan for this phase: "${phase.name}"`,
        ...(contextSection ? ["", contextSection] : [
          `Goal: ${phase.goal}`,
          `Requirements to cover: ${phase.requirements.join(", ") || "(none)"}`,
          `Success criteria: ${phase.successCriteria.join("; ") || "(none)"}`,
        ]),
        "",
        depthInstruction,
        "",
        "Return a ```json block with a PhasePlan: { steps: Array<{ id, description, subtasks: string[], dependsOn: string[] }>, dependencies: string[], acceptanceCriteria: string[] }",
      ].join("\n"),
      context:
        "You are a software implementation planner. Create a concrete, ordered plan to implement this phase.",
      timeoutSeconds: 120,
    };

    const raw = await this.dispatchTool.execute({ tasks: [task] }, toolContext);
    return this.parsePlanFromDispatch(raw, phase.id);
  }

  private async checkPlan(
    phase: PlanningPhase,
    plan: PhasePlan,
    toolContext: ToolContext,
    attempt: number,
    projectId?: string,
    projectGoal?: string,
  ): Promise<{ pass: boolean; issues: string[] }> {
    // Build context packet for reviewer — gives plan checker awareness of project context
    let contextSection = "";
    if (this.workspaceRoot && projectId) {
      try {
        contextSection = buildContextPacketSync({
          workspaceRoot: this.workspaceRoot,
          projectId,
          phaseId: phase.id,
          role: "reviewer",
          phase,
          projectGoal: projectGoal ?? "",
          requirements: this.store
            .listRequirements(projectId)
            .filter((r) => r.tier === "v1" && phase.requirements.includes(r.reqId)),
          maxTokens: 6000,
        });
      } catch (error) {
        log.warn(`Failed to build context packet for plan check: ${error instanceof Error ? error.message : String(error)}`);
      }
    }

    const taskParts = [
      `Review this implementation plan for phase "${phase.name}".`,
    ];

    if (contextSection) {
      taskParts.push("", contextSection);
    } else {
      taskParts.push(
        `Phase goal: ${phase.goal}`,
        `Phase requirements: ${phase.requirements.join(", ") || "(none)"}`,
      );
    }

    taskParts.push(
      "",
      `Plan:\n${JSON.stringify(plan, null, 2)}`,
      "",
      "Check: (1) Do the steps plausibly achieve the phase goal? (2) Are all phase REQ-IDs addressed?",
      "Return JSON: { pass: boolean, issues: string[] }",
      `This is check attempt ${attempt} of ${MAX_ITERATIONS}.`,
    );

    const task = {
      role: "reviewer",
      task: taskParts.join("\n"),
      context: "You are a plan quality reviewer. Be specific about any issues found.",
      timeoutSeconds: 60,
    };

    try {
      const raw = await this.dispatchTool.execute({ tasks: [task] }, toolContext);
      const dispatchOutput = JSON.parse(raw) as DispatchOutput;
      const result = dispatchOutput.results[0];

      if (!result || result.status !== "success" || !result.result) {
        // Best-effort: treat dispatch failure as pass to avoid blocking
        log.warn("Plan checker dispatch failed — treating as pass (best-effort)", { phaseId: phase.id, attempt });
        return { pass: true, issues: [] };
      }

      const parsed = this.extractCheckResult(result.result);
      return { pass: parsed.pass, issues: parsed.issues ?? [] };
    } catch (error) {
      log.warn("Plan checker parse error — treating as pass (best-effort)", { error, phaseId: phase.id, attempt });
      return { pass: true, issues: [] };
    }
  }

  private async revisePlan(
    phase: PlanningPhase,
    currentPlan: PhasePlan,
    issues: string[],
    depthInstruction: string,
    toolContext: ToolContext,
    projectId?: string,
    projectGoal?: string,
  ): Promise<PhasePlan> {
    // Build context packet for revision — planner needs full project awareness to fix issues
    let contextSection = "";
    if (this.workspaceRoot && projectId) {
      try {
        const allPhases = this.store.listPhases(projectId);
        contextSection = buildContextPacketSync({
          workspaceRoot: this.workspaceRoot,
          projectId,
          phaseId: phase.id,
          role: "planner",
          phase,
          allPhases,
          projectGoal: projectGoal ?? "",
          requirements: this.store
            .listRequirements(projectId)
            .filter((r) => r.tier === "v1" && phase.requirements.includes(r.reqId)),
          maxTokens: 8000,
        });
      } catch (error) {
        log.warn(`Failed to build context packet for plan revision: ${error instanceof Error ? error.message : String(error)}`);
      }
    }

    const taskParts = [
      `Revise this implementation plan for phase "${phase.name}".`,
    ];

    if (contextSection) {
      taskParts.push("", contextSection);
    } else {
      taskParts.push(
        `Phase goal: ${phase.goal}`,
        `Phase requirements: ${phase.requirements.join(", ") || "(none)"}`,
      );
    }

    taskParts.push(
      "",
      `Current plan:\n${JSON.stringify(currentPlan, null, 2)}`,
      "",
      `Issues to address:\n${issues.map((i) => `- ${i}`).join("\n")}`,
      "",
      depthInstruction,
      "",
      "Return a ```json block with the revised PhasePlan: { steps: Array<{ id, description, subtasks: string[], dependsOn: string[] }>, dependencies: string[], acceptanceCriteria: string[] }",
    );

    const task = {
      role: "planner",
      task: taskParts.join("\n"),
      context: "You are a software implementation planner. Revise the plan to address the reviewer's issues.",
      timeoutSeconds: 120,
    };

    const raw = await this.dispatchTool.execute({ tasks: [task] }, toolContext);
    return this.parsePlanFromDispatch(raw, phase.id);
  }

  private parsePlanFromDispatch(raw: string, phaseId: string): PhasePlan {
    const dispatchOutput = JSON.parse(raw) as DispatchOutput;
    const result = dispatchOutput.results[0];

    if (!result || result.status !== "success" || !result.result) {
      throw new PlanningError("Plan generation agent failed", {
        code: "PLANNING_STATE_CORRUPT",
        context: { phaseId, status: result?.status },
      });
    }

    const jsonMatch = /```json\s*([\s\S]*?)```/.exec(result.result);
    if (!jsonMatch?.[1]) {
      throw new PlanningError("Plan agent returned no JSON block", {
        code: "PLANNING_STATE_CORRUPT",
        context: { phaseId },
      });
    }

    return JSON.parse(jsonMatch[1]) as PhasePlan;
  }

  /** Extract JSON check result from sub-agent response that may include prose wrapping */
  private extractCheckResult(raw: string): { pass: boolean; issues: string[] } {
    // Try direct parse first
    try {
      return JSON.parse(raw) as { pass: boolean; issues: string[] };
    } catch {
      // Try extracting JSON from code block
      const codeBlockMatch = /```(?:json)?\s*([\s\S]*?)```/.exec(raw);
      if (codeBlockMatch?.[1]) {
        try {
          return JSON.parse(codeBlockMatch[1]) as { pass: boolean; issues: string[] };
        } catch { /* fall through */ }
      }

      // Try extracting JSON object from prose
      const jsonMatch = /\{[\s\S]*?"pass"\s*:[\s\S]*?\}/.exec(raw);
      if (jsonMatch) {
        try {
          return JSON.parse(jsonMatch[0]) as { pass: boolean; issues: string[] };
        } catch { /* fall through */ }
      }

      // If raw contains "pass" keyword heuristically
      if (/\bpass\b/i.test(raw) && !/\bfail\b/i.test(raw) && !/\bissue/i.test(raw)) {
        return { pass: true, issues: [] };
      }

      throw new Error(`Cannot extract check result from: ${raw.slice(0, 200)}`);
    }
  }
}
