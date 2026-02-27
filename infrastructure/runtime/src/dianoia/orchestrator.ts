// DianoiaOrchestrator — single state driver for all planning entry points
import { existsSync, readFileSync } from "node:fs";
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import {
  ensureProjectDir,
  writeDiscussFile,
  writePlanFile,
  writeProjectFile,
  writeRequirementsFile,
  writeResearchFile,
  writeRoadmapFile,
  writeVerifyFile,
} from "./project-files.js";
import { toSlug, isSlugTaken } from "./slug.js";
import {
  deriveMigrationSlug,
  detectLegacyProjectPaths,
  generateMigrationPrompt,
  migrateProjectToSlug,
} from "./migration.js";
import type { LegacyProject } from "./migration.js";
import { PlanningError } from "../koina/errors.js";
import type Database from "better-sqlite3";
import type { PlanningConfigSchema } from "../taxis/schema.js";
import type { DiscussionOption, DiscussionQuestion, PlanningProject, ProjectContext, RollbackAction, RollbackPlan, VerificationGap } from "./types.js";
import type { PhasePlan } from "./roadmap.js";
import { RetrospectiveGenerator } from "./retrospective.js";

const log = createLogger("dianoia:orchestrator");

const ACTIVE_STATES = new Set([
  "idle", "questioning", "researching",
  "requirements", "roadmap", "phase-planning",
  "executing", "verifying", "blocked",
]);

/**
 * Verify a file was written successfully - checks existence and non-empty content.
 * Throws PlanningError on failure for fail-fast behavior.
 */
function verifyFileWritten(filePath: string, fileType: string): void {
  if (!existsSync(filePath)) {
    throw new PlanningError(`${fileType} file was not written: ${filePath}`, {
      code: "FILE_NOT_FOUND",
      context: { filePath, fileType, reason: "file_not_found" }
    });
  }

  let content: string;
  try {
    content = readFileSync(filePath, "utf-8");
  } catch (error) {
    throw new PlanningError(`Failed to read ${fileType} file: ${filePath}`, {
      code: "FILE_PERMISSION_DENIED",
      context: { filePath, fileType },
      cause: error
    });
  }

  if (content.length === 0) {
    throw new PlanningError(`${fileType} file is empty: ${filePath}`, {
      code: "PLANNING_STATE_CORRUPT", 
      context: { filePath, fileType, reason: "empty_file" }
    });
  }
}

export class DianoiaOrchestrator {
  private store: PlanningStore;
  private retroGenerator: RetrospectiveGenerator;
  /** Three-phase slug intake: null = not started, "" = waiting for name, string = name received */
  private pendingProjectName: string | null = null;
  /** Set after name received — shown to user for confirmation */
  private pendingSlug: string | null = null;
  /**
   * Migration sentinel:
   *   null     = not yet checked this process lifetime (initial value, resets on restart)
   *   []       = checked this session — declined or no legacy paths found; prevents re-prompt within session
   *   [...arr] = legacy paths detected, awaiting user response
   */
  private pendingMigration: LegacyProject[] | null = null;

  constructor(private db: Database.Database, private defaultConfig: PlanningConfigSchema) {
    this.store = new PlanningStore(db);
    this.retroGenerator = new RetrospectiveGenerator(db);
  }

  private getDb(): Database.Database {
    return this.db;
  }

  /** No-op — kept for call-site compatibility during migration */
  setWorkspaceRoot(_root: string): void { /* workspace root now resolved from project.projectDir via getProjectDir() */ }

  handle(nousId: string, _sessionId: string): string {
    // Check for legacy migration (only once per process lifetime, before anything else)
    if (this.pendingMigration === null) {
      const legacy = detectLegacyProjectPaths(this.getDb());
      if (legacy.length > 0) {
        this.pendingMigration = legacy;
        return generateMigrationPrompt(legacy);
      }
      this.pendingMigration = []; // empty array = checked, none found
    }

    // If migration is pending (user hasn't responded yet), re-surface the prompt
    if (this.pendingMigration.length > 0) {
      return generateMigrationPrompt(this.pendingMigration);
    }

    const active = this.getActiveProject(nousId);

    if (active) {
      if (this.hasPendingConfirmation(active)) {
        return this.prependProjectContext(
          `Still working on "${active.goal || "your project"}"? (yes to resume, no to start fresh)`,
          active,
        );
      }
      const updated = { ...(active.config as Record<string, unknown>), pendingConfirmation: true };
      this.store.updateProjectConfig(active.id, updated as unknown as PlanningConfigSchema);
      return this.prependProjectContext(
        `Still working on "${active.goal || "your project"}"? (yes to resume, no to start fresh)`,
        active,
      );
    }

    // Three-phase slug intake: ask name → confirm slug → create project
    if (this.pendingProjectName === null) {
      this.pendingProjectName = "";
      return 'Starting a new planning project. What should we call this project? (e.g. "My Aletheia Plugin")';
    }

    // Name received, slug confirmation in progress
    if (this.pendingSlug !== null) {
      return `Your slug will be: ${this.pendingSlug}\nPress Enter to confirm, or type a different slug:`;
    }

    // Waiting for name (pendingProjectName is "")
    return 'What should we call this project? (e.g. "My Aletheia Plugin")';
  }

  /** True when we have asked for a project name but not yet received a valid one */
  hasPendingNameIntake(): boolean {
    return this.pendingProjectName !== null && this.pendingProjectName === "";
  }

  /** True when a slug has been generated and is awaiting user confirmation */
  hasPendingSlugConfirmation(): boolean {
    return this.pendingSlug !== null;
  }

  /** True when legacy paths were detected and the user has not yet responded this session */
  hasPendingMigration(): boolean {
    return this.pendingMigration !== null && this.pendingMigration.length > 0;
  }

  /**
   * Handle a migration confirmation response ("yes" / "not now").
   *
   * On "yes": migrates all legacy projects to slug paths, sets pendingMigration = [].
   * On "not now": sets pendingMigration = [] (empty array — NOT null).
   *   - Empty array prevents re-prompting within the same session.
   *   - null would re-trigger detectLegacyProjectPaths() on the next handle() call.
   *   - Prompt re-appears on next startup because the constructor always initializes to null.
   */
  handleMigrationResponse(answer: string, nousId: string, sessionId: string): string {
    if (!this.pendingMigration || this.pendingMigration.length === 0) {
      return this.handle(nousId, sessionId); // no pending migration — proceed normally
    }

    const normalizedAnswer = answer.trim().toLowerCase();

    if (normalizedAnswer === "yes" || normalizedAnswer === "y") {
      const results: string[] = [];
      for (const project of this.pendingMigration) {
        const slug = deriveMigrationSlug(project, this.getDb());
        try {
          migrateProjectToSlug(project, slug, this.getDb());
          results.push(`  - "${project.goal || project.id}" migrated to ${slug}`);
        } catch (err) {
          results.push(
            `  - "${project.goal || project.id}" — migration failed: ${err instanceof Error ? err.message : String(err)}`,
          );
        }
      }
      this.pendingMigration = []; // empty array = done for this session
      return [
        "Migration complete:",
        ...results,
        "",
        "Projects now stored at _shared/workspace/plans/{slug}/.",
      ].join("\n");
    }

    if (
      normalizedAnswer === "not now" ||
      normalizedAnswer === "no" ||
      normalizedAnswer === "n"
    ) {
      // User declined — set to EMPTY ARRAY (not null) so same-session handle() calls skip detection.
      // null would re-trigger detectLegacyProjectPaths() on the very next handle() call.
      // Re-prompting on next startup works because the constructor always initializes to null.
      this.pendingMigration = [];
      return "Migration skipped. Projects continue working at their current paths. You'll be prompted again on next startup.";
    }

    // Unrecognized response — re-surface the prompt
    return `Please respond with "yes" to migrate or "not now" to skip.\n\n${generateMigrationPrompt(this.pendingMigration)}`;
  }

  /** Called when user responds while hasPendingNameIntake() is true */
  receiveProjectName(name: string, _nousId: string, _sessionId: string): string {
    const trimmed = name.trim();
    if (!trimmed) return "Project name is required. Please provide a name.";

    const slug = toSlug(trimmed);
    if (!slug) return "Could not generate a valid slug from that name. Please use letters and numbers.";

    if (isSlugTaken(slug, this.getDb())) {
      return `Slug "${slug}" is already taken by an existing project. Choose a different name.`;
    }

    this.pendingProjectName = trimmed;
    this.pendingSlug = slug;
    return `Your slug will be: ${slug}\nPress Enter to confirm, or type a different slug:`;
  }

  /** Called when user responds while hasPendingSlugConfirmation() is true */
  receiveSlugConfirmation(answer: string, nousId: string, sessionId: string): string {
    const trimmed = answer.trim();

    // Empty input or explicit Enter = confirm current slug
    const chosenSlug = trimmed === "" ? this.pendingSlug! : trimmed;
    const normalizedSlug = toSlug(chosenSlug);

    if (!normalizedSlug) {
      return `Could not generate a valid slug from "${chosenSlug}". Please use letters and numbers.`;
    }

    if (normalizedSlug !== this.pendingSlug && isSlugTaken(normalizedSlug, this.getDb())) {
      return `Slug "${normalizedSlug}" is already taken. Choose a different slug:`;
    }

    return this.createProjectWithSlug(this.pendingProjectName!, normalizedSlug, nousId, sessionId);
  }

  private createProjectWithSlug(displayName: string, slug: string, nousId: string, sessionId: string): string {
    this.pendingProjectName = null;
    this.pendingSlug = null;

    const project = this.store.createProject({
      nousId,
      sessionId,
      goal: "",
      config: this.defaultConfig,
    });
    this.store.updateProjectState(project.id, transition("idle", "START_QUESTIONING"));

    // ensureProjectDir may throw if paths not initialized (test env) — store slug regardless
    let dir: string | null = null;
    try {
      dir = ensureProjectDir(slug);
    } catch {
      /* paths not initialized — project dir will be resolved on first file access */
    }
    this.store.updateProjectDir(project.id, slug);

    eventBus.emit("planning:project-created", { projectId: project.id, nousId, sessionId });
    log.info(`Created planning project ${project.id} for nous ${nousId}`, { slug, dir });
    return `Project "${displayName}" (slug: ${slug}) created. Artifacts will be stored in _shared/workspace/plans/${slug}/\n\nFirst: what are you building?`;
  }

  confirmResume(projectId: string, nousId: string, sessionId: string, answer: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    const cleared = { ...(project.config as Record<string, unknown>), pendingConfirmation: false };
    this.store.updateProjectConfig(projectId, cleared as unknown as PlanningConfigSchema);

    if (answer.toLowerCase().includes("yes") || answer.toLowerCase() === "y") {
      eventBus.emit("planning:project-resumed", { projectId, nousId, sessionId });
      log.info(`Resumed planning project ${projectId} for nous ${nousId}`);
      return this.prependProjectContext(
        `Resuming your planning project. You're in the ${project.state} phase.`,
        project,
      );
    }

    this.abandon(projectId);
    // Reset intake state so handle() starts fresh with the name prompt
    this.pendingProjectName = null;
    this.pendingSlug = null;
    log.info(`Abandoned project ${projectId} for nous ${nousId}; ready for new project`);
    return 'Starting a new planning project. What should we call this project? (e.g. "My Aletheia Plugin")';
  }

  abandon(projectId: string): void {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "ABANDON"));

    // Generate retrospective even on abandon — failure patterns are valuable
    this.generateRetro(projectId);

    eventBus.emit("planning:state-transition", { projectId, from: project.state, to: "abandoned" });
    log.info(`Abandoned planning project ${projectId}`);
  }

  getActiveProject(nousId: string): PlanningProject | undefined {
    return this.store.listProjects(nousId).find((p) => ACTIVE_STATES.has(p.state));
  }

  hasPendingConfirmation(project: PlanningProject): boolean {
    return (project.config as Record<string, unknown>)["pendingConfirmation"] === true;
  }

  // Question sequence for gathering project context — adaptive, stops when enough gathered
  private static readonly QUESTIONS = [
    "What are you building, and why now? (goal + motivation)",
    "What are the hard constraints — technology stack, compatibility, time, or team?",
    "What architectural or approach decisions have you already made?",
    "Who are the primary users, and what's the one thing this must do well?",
    "Any integration dependencies or external services involved?",
  ];

  processAnswer(projectId: string, userText: string): void {
    const project = this.store.getProjectOrThrow(projectId);
    if (project.state !== "questioning") return;

    const existing = project.projectContext ?? {};
    const transcript = existing.rawTranscript ?? [];
    transcript.push({ turn: transcript.length + 1, text: userText });
    this.store.updateProjectContext(projectId, { ...existing, rawTranscript: transcript });
  }

  getNextQuestion(projectId: string): string | null {
    const project = this.store.getProjectOrThrow(projectId);
    if (project.state !== "questioning") return null;

    const answered = project.projectContext?.rawTranscript?.length ?? 0;
    if (answered >= DianoiaOrchestrator.QUESTIONS.length) return null;
    return DianoiaOrchestrator.QUESTIONS[answered] ?? null;
  }

  synthesizeContext(projectId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    const transcript = project.projectContext?.rawTranscript ?? [];
    const lines = transcript.map((t) => `- ${t.text}`).join("\n");
    return `Here's what I captured:\n\n${lines || "(no answers recorded)"}\n\nDoes this look right? (yes to continue, or tell me what to change)`;
  }

  confirmSynthesis(
    projectId: string,
    nousId: string,
    sessionId: string,
    synthesizedContext: ProjectContext,
  ): string {
    const project = this.store.getProjectOrThrow(projectId);

    const existing = project.projectContext ?? {};
    const rawTranscript = existing.rawTranscript;
    const merged: ProjectContext = {
      ...existing,
      ...synthesizedContext,
      ...(rawTranscript !== undefined ? { rawTranscript } : {}),
    };

    if (synthesizedContext.goal) {
      this.store.updateProjectGoal(projectId, synthesizedContext.goal);
    }
    this.store.updateProjectContext(projectId, merged);

    // Transition FSM: questioning -> researching (event: START_RESEARCH)
    this.store.updateProjectState(projectId, transition("questioning", "START_RESEARCH"));

    // Write PROJECT.md with confirmed context
    const updated = this.store.getProjectOrThrow(projectId);
    if (updated.projectDir) {
      try {
        writeProjectFile(updated, merged);

        // Verify PROJECT.md was written successfully
        const projectPath = `${updated.projectDir}/PROJECT.md`;
        verifyFileWritten(projectPath, "PROJECT.md");
      } catch (error) {
        // Skip if paths not initialized (test env or paths not yet set up)
        log.warn(`Could not write PROJECT.md for ${projectId}: ${error instanceof Error ? error.message : String(error)}`);
      }
    }

    eventBus.emit("planning:phase-started", {
      projectId,
      nousId,
      sessionId,
      fromState: "questioning",
      toState: "researching",
    });

    log.info(`Context confirmed for project ${projectId}; advancing to researching state`);
    return this.prependProjectContext("Context saved. Moving to research phase.", updated);
  }

  listAllProjects(): PlanningProject[] {
    return this.store.listProjects();
  }

  getProject(id: string): PlanningProject | undefined {
    return this.store.getProject(id);
  }

  skipResearch(projectId: string, nousId: string, sessionId: string): string {
    this.store.updateProjectState(projectId, transition("researching", "RESEARCH_COMPLETE"));

    // Write RESEARCH.md even if skipped (records what was available)
    const project = this.store.getProject(projectId);
    if (project?.projectDir) {
      const research = this.store.listResearch(projectId);
      if (research.length > 0) writeResearchFile(project.projectDir, research);
    }

    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "research" });
    log.info(`Research skipped for project ${projectId}; advancing to requirements`);
    return this.prependProjectContext("Research skipped. Proceeding to requirements definition.", project ?? null);
  }

  completeRequirements(projectId: string, nousId: string, sessionId: string): string {
    this.store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"));

    // Write REQUIREMENTS.md
    const project = this.store.getProjectOrThrow(projectId);
    if (project.projectDir) {
      const reqs = this.store.listRequirements(projectId);
      writeRequirementsFile(project.projectDir, reqs);

      // Verify REQUIREMENTS.md was written successfully
      const requirementsPath = `${project.projectDir}/REQUIREMENTS.md`;
      verifyFileWritten(requirementsPath, "REQUIREMENTS.md");
    }

    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "requirements" });
    log.info(`Requirements complete for project ${projectId}; advancing to roadmap`);
    return this.prependProjectContext("Requirements confirmed. Advancing to roadmap generation.", project);
  }

  completePhase(projectId: string, nousId: string, sessionId: string, phase: string): void {
    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase });
    log.info(`Phase complete: ${phase} for project ${projectId}`);
  }

  completeProject(projectId: string, nousId: string, sessionId: string): void {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(
      projectId,
      transition(project.state, "ALL_PHASES_COMPLETE"),
    );

    // Generate retrospective on project completion
    this.generateRetro(projectId);

    eventBus.emit("planning:complete", { projectId, nousId, sessionId });
    log.info(`Project complete: ${projectId}`);
  }

  completeAllPhases(projectId: string, nousId: string, sessionId: string): void {
    this.store.updateProjectState(projectId, transition("verifying", "ALL_PHASES_COMPLETE"));

    // Generate retrospective on project completion
    this.generateRetro(projectId);

    eventBus.emit("planning:complete", { projectId, nousId, sessionId });
    log.info("All phases complete", { projectId });
  }

  // --- Roadmap, Execution, Verification ---

  completeRoadmap(projectId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "ROADMAP_COMPLETE"));

    // Write ROADMAP.md
    if (project.projectDir) {
      const phases = this.store.listPhases(projectId);
      writeRoadmapFile(project.projectDir, phases);

      // Verify ROADMAP.md was written successfully
      const roadmapPath = `${project.projectDir}/ROADMAP.md`;
      verifyFileWritten(roadmapPath, "ROADMAP.md");
    }

    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "roadmap" });
    log.info(`Roadmap complete for project ${projectId}; advancing to discussion`);
    return this.prependProjectContext("Roadmap complete. Moving to phase discussion.", project);
  }

  advanceToExecution(projectId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "PLAN_READY"));
    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "phase-planning" });
    log.info(`Advancing to execution for project ${projectId}`);
    return "Plan ready. Advancing to execution.";
  }

  advanceToVerification(projectId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "VERIFY"));
    eventBus.emit("planning:state-transition", { projectId, nousId, sessionId, from: project.state, to: "verifying" });
    log.info(`Advancing to verification for project ${projectId}`);
    return "Execution complete. Moving to verification.";
  }

  advanceToNextPhase(projectId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "NEXT_PHASE"));
    eventBus.emit("planning:phase-started", { projectId, nousId, sessionId });
    log.info(`Next phase started for project ${projectId}`);
    return "Moving to next phase.";
  }

  blockOnVerificationFailure(projectId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "PHASE_FAILED"));
    eventBus.emit("planning:state-transition", { projectId, nousId, sessionId, from: project.state, to: "blocked" });
    log.info(`Verification failed for project ${projectId} — blocked`);
    return "Verification failed. Project is blocked pending gap closure.";
  }

  /**
   * ORCH-04: Auto-skip downstream phases on verification failure and generate rollback plan
   * Called when a phase fails verification to cascade-skip dependent phases
   */
  skipDownstreamPhasesOnVerificationFailure(
    projectId: string, 
    failedPhaseId: string, 
    verificationGaps: VerificationGap[]
  ): { skippedPhases: string[], rollbackPlan: RollbackPlan } {
    const allPhases = this.store.listPhases(projectId);
    const dependentPhases = this.findDirectDependentPhases(failedPhaseId, allPhases);
    
    // Skip direct dependents only (following execution.ts pattern)
    const skippedPhases: string[] = [];
    for (const phase of dependentPhases) {
      if (phase.status === "pending" || phase.status === "executing") {
        this.store.updatePhaseStatus(phase.id, "skipped");
        skippedPhases.push(phase.id);
        log.info(`Skipped phase ${phase.id} (${phase.name}) due to failed dependency ${failedPhaseId}`);
      }
    }

    // Generate rollback plan from verification gaps
    const rollbackPlan = this.generateRollbackPlan(failedPhaseId, verificationGaps, allPhases);

    log.info(`Verification failure cascade: skipped ${skippedPhases.length} dependent phases for ${failedPhaseId}`);
    
    return { skippedPhases, rollbackPlan };
  }

  /**
   * Find phases that directly depend on the failed phase
   * Mirrors directDependents logic from execution.ts
   */
  private findDirectDependentPhases(failedPhaseId: string, allPhases: import("./types.js").PlanningPhase[]) {
    return allPhases.filter((phase) => {
      const plan = phase.plan as PhasePlan | null;
      const dependencies = plan?.dependencies ?? [];
      return dependencies.includes(failedPhaseId);
    });
  }

  /**
   * Generate rollback plan that surfaces verification gaps as concrete actions
   */
  private generateRollbackPlan(
    failedPhaseId: string, 
    gaps: VerificationGap[], 
    allPhases: import("./types.js").PlanningPhase[]
  ): RollbackPlan {
    const failedPhase = allPhases.find(p => p.id === failedPhaseId);
    const phaseName = failedPhase?.name ?? "Unknown Phase";

    const actions: RollbackAction[] = gaps.map((gap, index) => ({
      id: `gap-${index + 1}`,
      type: "fix-verification-gap",
      description: gap.criterion ?? "Unknown criterion",
      detail: gap.detail ?? "No details provided", 
      proposedFix: gap.proposedFix ?? "Manual review required",
      priority: gap.status === "not-met" ? "high" : "medium"
    }));

    // Add phase completion action
    actions.push({
      id: "rerun-verification", 
      type: "verify-phase",
      description: `Re-run verification for ${phaseName}`,
      detail: `After addressing gaps, verify phase ${failedPhaseId} meets all success criteria`,
      proposedFix: "Use plan_verify tool with action=run",
      priority: "high"
    });

    return {
      failedPhaseId,
      phaseName,
      failureReason: `Verification failed: ${gaps.length} gaps found`,
      gapCount: gaps.length,
      actions,
      estimatedEffort: this.estimateRollbackEffort(gaps),
      createdAt: new Date().toISOString()
    };
  }

  /**
   * Estimate effort required to address verification gaps
   */
  private estimateRollbackEffort(gaps: VerificationGap[]): "low" | "medium" | "high" {
    const criticalGaps = gaps.filter(g => g.status === "not-met").length;
    const partialGaps = gaps.filter(g => g.status === "partially-met").length;
    
    if (criticalGaps > 2) return "high";
    if (criticalGaps > 0 || partialGaps > 3) return "medium";
    return "low";
  }

  pauseExecution(projectId: string): string {
    this.store.getProjectOrThrow(projectId); // validate exists
    log.info(`Execution paused for project ${projectId}`);
    return `Execution paused for project ${projectId}. Resume with resumeExecution().`;
  }

  resumeExecution(projectId: string, _nousId: string, _sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    if (project.state === "blocked") {
      this.store.updateProjectState(projectId, transition(project.state, "RESUME"));
    }
    log.info(`Execution resumed for project ${projectId}`);
    return `Execution resumed for project ${projectId}.`;
  }

  listPhases(projectId: string): import("./types.js").PlanningPhase[] {
    return this.store.listPhases(projectId);
  }

  updateGoal(projectId: string, goal: string): void {
    this.store.updateProjectGoal(projectId, goal);
  }

  updateContext(projectId: string, context: ProjectContext): void {
    this.store.updateProjectContext(projectId, context);
  }

  // --- Retrospective (Spec 32 Phase 4) ---

  /** Generate and persist retrospective for a project */
  private generateRetro(projectId: string): void {
    try {
      const retro = this.retroGenerator.generate(projectId);
      const project = this.store.getProject(projectId);
      if (project?.projectDir) {
        this.retroGenerator.writeRetroFile(project.projectDir, retro);
        this.retroGenerator.writeRetroJson(project.projectDir, retro);
      }
      log.info(`Retrospective generated for project ${projectId}: ${retro.patterns.length} patterns`);
    } catch (error) {
      // Don't let retro failure block project state transitions
      log.warn(`Failed to generate retrospective for ${projectId}`, { error });
    }
  }

  /** Explicitly generate retrospective (e.g., for mid-project review) */
  generateRetrospective(projectId: string): import("./retrospective.js").RetrospectiveEntry {
    return this.retroGenerator.generate(projectId);
  }

  // --- Discussion flow (Spec 32) ---

  /** Create a discussion question for a phase */
  addDiscussionQuestion(
    projectId: string,
    phaseId: string,
    question: string,
    options: DiscussionOption[],
    recommendation?: string | null,
  ): DiscussionQuestion {
    return this.store.createDiscussionQuestion({
      projectId,
      phaseId,
      question,
      options,
      recommendation: recommendation ?? null,
    });
  }

  /** Answer a discussion question */
  answerDiscussion(questionId: string, decision: string, userNote?: string | null): void {
    this.store.answerDiscussionQuestion(questionId, decision, userNote);
  }

  /** Skip a discussion question (agent uses its recommendation) */
  skipDiscussion(questionId: string): void {
    this.store.skipDiscussionQuestion(questionId);
  }

  /** Get pending questions for a phase */
  getPendingDiscussions(projectId: string, phaseId: string): DiscussionQuestion[] {
    return this.store.getPendingDiscussionQuestions(projectId, phaseId);
  }

  /** Get all discussion questions for a phase */
  getPhaseDiscussions(projectId: string, phaseId: string): DiscussionQuestion[] {
    return this.store.listDiscussionQuestions(projectId, phaseId);
  }

  /** Complete discussion phase — writes DISCUSS.md and advances to planning.
   *  Idempotent: if project is already in phase-planning (another phase's discussion
   *  already completed), skips the FSM transition but still writes the DISCUSS.md file. */
  completeDiscussion(projectId: string, phaseId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);

    // Only transition if still in 'discussing' — subsequent phase discussions
    // complete while project is already in 'phase-planning', which is fine
    if (project.state === "discussing") {
      this.store.updateProjectState(projectId, transition(project.state, "DISCUSSION_COMPLETE"));
    } else if (project.state !== "phase-planning") {
      // Unexpected state — let the FSM throw so we don't silently corrupt
      this.store.updateProjectState(projectId, transition(project.state, "DISCUSSION_COMPLETE"));
    }

    // Write DISCUSS.md with all questions and decisions
    const projectForDiscuss = this.store.getProject(projectId);
    if (projectForDiscuss?.projectDir) {
      const questions = this.store.listDiscussionQuestions(projectId, phaseId);
      writeDiscussFile(projectForDiscuss.projectDir, phaseId, questions);
    }

    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "discussing" });
    log.info(`Discussion complete for phase ${phaseId} in project ${projectId}; advancing to planning`);
    return "Discussion complete. Advancing to phase planning.";
  }

  /** Advance to next phase — now goes to discussing instead of phase-planning */
  advanceToNextPhaseDiscussion(projectId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "NEXT_PHASE"));
    eventBus.emit("planning:phase-started", { projectId, nousId, sessionId });
    log.info(`Next phase discussion started for project ${projectId}`);
    return "Moving to discussion for next phase.";
  }

  // --- File sync helpers ---

  /** Write/update the PROJECT.md file with current state */
  syncProjectFile(projectId: string): void {
    const project = this.store.getProjectOrThrow(projectId);
    if (!project.projectDir) return;
    writeProjectFile(project);
  }

  /** Write execution plan file for a phase */
  syncPlanFile(projectId: string, phaseId: string, plan: unknown): void {
    const project = this.store.getProject(projectId);
    if (!project?.projectDir) return;
    writePlanFile(project.projectDir, phaseId, plan);
  }

  /** Write verification results for a phase */
  syncVerifyFile(projectId: string, phaseId: string, verification: Record<string, unknown>): void {
    const project = this.store.getProject(projectId);
    if (!project?.projectDir) return;
    writeVerifyFile(project.projectDir, phaseId, verification);
  }

  /**
   * Prepend [Project: {slug}] to a response when a project is active.
   * For legacy absolute paths, extracts the last path segment as a display label.
   */
  private prependProjectContext(response: string, project: PlanningProject | null): string {
    if (!project?.projectDir) return response;
    const label = project.projectDir.startsWith("/")
      ? (project.projectDir.split("/").at(-1) ?? project.projectDir)
      : project.projectDir;
    return `[Project: ${label}]\n\n${response}`;
  }
}
