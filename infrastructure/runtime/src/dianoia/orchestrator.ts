// DianoiaOrchestrator — single state driver for all planning entry points
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import type Database from "better-sqlite3";
import type { PlanningConfigSchema } from "../taxis/schema.js";
import type { PlanningProject, ProjectContext } from "./types.js";

const log = createLogger("dianoia:orchestrator");

const ACTIVE_STATES = new Set([
  "idle", "questioning", "researching",
  "requirements", "roadmap", "phase-planning",
  "executing", "verifying", "blocked",
]);

export class DianoiaOrchestrator {
  private store: PlanningStore;

  constructor(db: Database.Database, private defaultConfig: PlanningConfigSchema) {
    this.store = new PlanningStore(db);
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
    eventBus.emit("planning:phase-complete", { projectId, nousId, sessionId, phase: "research" });
    log.info(`Research skipped for project ${projectId}; advancing to requirements`);
    return "Research skipped. Proceeding to requirements definition.";
  }

  completeRequirements(projectId: string, nousId: string, sessionId: string): string {
    this.store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"));
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
    eventBus.emit("planning:complete", { projectId, nousId, sessionId });
    log.info(`Project complete: ${projectId}`);
  }
}
