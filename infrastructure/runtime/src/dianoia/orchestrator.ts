// DianoiaOrchestrator — single state driver for all planning entry points
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import {
  ensureProjectDir,
  writeProjectFile,
  writeRequirementsFile,
  writeResearchFile,
  writeRoadmapFile,
  writeDiscussFile,
  writePlanFile,
  writeVerifyFile,
} from "./project-files.js";
import type Database from "better-sqlite3";
import type { PlanningConfigSchema } from "../taxis/schema.js";
import type { DiscussionOption, DiscussionQuestion, PlanningProject, ProjectContext } from "./types.js";
import { RetrospectiveGenerator } from "./retrospective.js";

const log = createLogger("dianoia:orchestrator");

const ACTIVE_STATES = new Set([
  "idle", "questioning", "researching",
  "requirements", "roadmap", "phase-planning",
  "executing", "verifying", "blocked",
]);

export class DianoiaOrchestrator {
  private store: PlanningStore;
  private retroGenerator: RetrospectiveGenerator;
  private workspaceRoot: string | null = null;

  constructor(db: Database.Database, private defaultConfig: PlanningConfigSchema) {
    this.store = new PlanningStore(db);
    this.retroGenerator = new RetrospectiveGenerator(db);
  }

  /** Set the workspace root for file-backed state. Must be called before file writes work. */
  setWorkspaceRoot(root: string): void {
    this.workspaceRoot = root;
  }

  getWorkspaceOrThrow(): string {
    if (!this.workspaceRoot) {
      throw new Error("DianoiaOrchestrator: workspaceRoot not set — call setWorkspaceRoot() first");
    }
    return this.workspaceRoot;
  }

  handle(nousId: string, sessionId: string): string {
    const active = this.getActiveProject(nousId);

    if (active) {
      if (this.hasPendingConfirmation(active)) {
        return `Still working on "${active.goal || "your project"}"? (yes to resume, no to start fresh)`;
      }
      const updated = { ...(active.config as Record<string, unknown>), pendingConfirmation: true };
      this.store.updateProjectConfig(active.id, updated as unknown as PlanningConfigSchema);
      return `Still working on "${active.goal || "your project"}"? (yes to resume, no to start fresh)`;
    }

    const project = this.store.createProject({
      nousId,
      sessionId,
      goal: "",
      config: this.defaultConfig,
    });
    this.store.updateProjectState(project.id, transition("idle", "START_QUESTIONING"));

    // Set up file-backed state directory
    if (this.workspaceRoot) {
      const dir = ensureProjectDir(this.workspaceRoot, project.id);
      this.store.updateProjectDir(project.id, dir);
    }

    eventBus.emit("planning:project-created", { projectId: project.id, nousId, sessionId });
    log.info(`Created planning project ${project.id} for nous ${nousId}`);
    return "Starting a Dianoia planning project. First: what are you building?";
  }

  confirmResume(projectId: string, nousId: string, sessionId: string, answer: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    const cleared = { ...(project.config as Record<string, unknown>), pendingConfirmation: false };
    this.store.updateProjectConfig(projectId, cleared as unknown as PlanningConfigSchema);

    if (answer.toLowerCase().includes("yes") || answer.toLowerCase() === "y") {
      eventBus.emit("planning:project-resumed", { projectId, nousId, sessionId });
      log.info(`Resumed planning project ${projectId} for nous ${nousId}`);
      return `Resuming your planning project. You're in the ${project.state} phase.`;
    }

    this.abandon(projectId);
    const newProject = this.store.createProject({
      nousId,
      sessionId,
      goal: "",
      config: this.defaultConfig,
    });
    this.store.updateProjectState(newProject.id, transition("idle", "START_QUESTIONING"));
    eventBus.emit("planning:project-created", { projectId: newProject.id, nousId, sessionId });
    log.info(`Started fresh planning project ${newProject.id} for nous ${nousId}`);
    return "Starting a Dianoia planning project. First: what are you building?";
  }

  abandon(projectId: string): void {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "ABANDON"));

    // Generate retrospective even on abandon — failure patterns are valuable
    this.generateRetro(projectId);

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
    if (this.workspaceRoot) {
      const updated = this.store.getProjectOrThrow(projectId);
      writeProjectFile(this.workspaceRoot, updated, merged);
    }

    eventBus.emit("planning:phase-started", {
      projectId,
      nousId,
      sessionId,
      fromState: "questioning",
      toState: "researching",
    });

    log.info(`Context confirmed for project ${projectId}; advancing to researching state`);
    return "Context saved. Moving to research phase.";
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
    if (this.workspaceRoot) {
      const research = this.store.listResearch(projectId);
      if (research.length > 0) writeResearchFile(this.workspaceRoot, projectId, research);
    }

    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "research" });
    log.info(`Research skipped for project ${projectId}; advancing to requirements`);
    return "Research skipped. Proceeding to requirements definition.";
  }

  completeRequirements(projectId: string, nousId: string, sessionId: string): string {
    this.store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"));

    // Write REQUIREMENTS.md
    if (this.workspaceRoot) {
      const reqs = this.store.listRequirements(projectId);
      writeRequirementsFile(this.workspaceRoot, projectId, reqs);
    }

    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "requirements" });
    log.info(`Requirements complete for project ${projectId}; advancing to roadmap`);
    return "Requirements confirmed. Advancing to roadmap generation.";
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
    if (this.workspaceRoot) {
      const phases = this.store.listPhases(projectId);
      writeRoadmapFile(this.workspaceRoot, projectId, phases);
    }

    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "roadmap" });
    log.info(`Roadmap complete for project ${projectId}; advancing to discussion`);
    return "Roadmap complete. Moving to phase discussion.";
  }

  advanceToExecution(projectId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "PLAN_READY"));
    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "phase-planning" });
    log.info(`Advancing to execution for project ${projectId}`);
    return "Plan ready. Advancing to execution.";
  }

  advanceToVerification(projectId: string, _nousId: string, _sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "VERIFY"));
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

  blockOnVerificationFailure(projectId: string, _nousId: string, _sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "PHASE_FAILED"));
    log.info(`Verification failed for project ${projectId} — blocked`);
    return "Verification failed. Project is blocked pending gap closure.";
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
      if (this.workspaceRoot) {
        this.retroGenerator.writeRetroFile(this.workspaceRoot, retro);
        this.retroGenerator.writeRetroJson(this.workspaceRoot, retro);
      }
      log.info(`Retrospective generated for project ${projectId}: ${retro.patterns.length} patterns`);
    } catch (err) {
      // Don't let retro failure block project state transitions
      log.warn(`Failed to generate retrospective for ${projectId}`, { err });
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

  /** Complete discussion phase — writes DISCUSS.md and advances to planning */
  completeDiscussion(projectId: string, phaseId: string, nousId: string, sessionId: string): string {
    const project = this.store.getProjectOrThrow(projectId);
    this.store.updateProjectState(projectId, transition(project.state, "DISCUSSION_COMPLETE"));

    // Write DISCUSS.md with all questions and decisions
    if (this.workspaceRoot) {
      const questions = this.store.listDiscussionQuestions(projectId, phaseId);
      writeDiscussFile(this.workspaceRoot, projectId, phaseId, questions);
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
    if (!this.workspaceRoot) return;
    const project = this.store.getProjectOrThrow(projectId);
    writeProjectFile(this.workspaceRoot, project);
  }

  /** Write execution plan file for a phase */
  syncPlanFile(projectId: string, phaseId: string, plan: unknown): void {
    if (!this.workspaceRoot) return;
    writePlanFile(this.workspaceRoot, projectId, phaseId, plan);
  }

  /** Write verification results for a phase */
  syncVerifyFile(projectId: string, phaseId: string, verification: Record<string, unknown>): void {
    if (!this.workspaceRoot) return;
    writeVerifyFile(this.workspaceRoot, projectId, phaseId, verification);
  }
}
