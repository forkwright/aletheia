// DianoiaOrchestrator — single state driver for all planning entry points
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import type Database from "better-sqlite3";
import type { PlanningConfigSchema } from "../taxis/schema.js";
import type { PlanningProject } from "./types.js";

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
}
