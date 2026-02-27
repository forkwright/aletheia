// file-sync.ts — Event-driven file synchronization for co-primary state (Spec 32 Phase 1)
//
// Listens to planning events on the event bus and writes corresponding files.
// This ensures files stay in sync with DB without modifying every callsite.
// Files are the recovery path — if DB is lost, files + reconciler rebuild state.

import { createLogger } from "../koina/logger.js";
import { eventBus, type EventName, type EventHandler } from "../koina/event-bus.js";
import { PlanningStore } from "./store.js";
import {
  ensureProjectDir,
  ensurePhaseDir,
  writeProjectFile,
  writeRequirementsFile,
  writeRoadmapFile,
  writeResearchFile,
  writeDiscussFile,
  writePlanFile,
  writeStateFile,
  writeVerifyFile,
} from "./project-files.js";
import type Database from "better-sqlite3";

const log = createLogger("dianoia:file-sync");

/**
 * FileSyncDaemon — subscribes to planning events and writes files alongside DB.
 *
 * Every planning mutation already emits events via eventBus. This daemon
 * catches them and ensures the file tree stays current. If a file write fails,
 * it logs but doesn't block the DB write — the reconciler fixes it on next startup.
 *
 * This replaces the ad-hoc file writes scattered across orchestrator methods.
 * Those remain as a belt-and-suspenders measure, but this daemon is the primary
 * file-write path going forward.
 */
export class FileSyncDaemon {
  private store: PlanningStore;
  private workspaceRoot: string | null = null;
  private listeners: Array<() => void> = [];
  private writeCount = 0;
  private errorCount = 0;

  constructor(db: Database.Database) {
    this.store = new PlanningStore(db);
  }

  /** Set workspace root and start listening for events */
  start(workspaceRoot: string): void {
    this.workspaceRoot = workspaceRoot;
    this.subscribe();
    log.info("FileSyncDaemon started", { workspaceRoot });
  }

  /** Stop listening */
  stop(): void {
    for (const unsub of this.listeners) unsub();
    this.listeners = [];
    log.info("FileSyncDaemon stopped", { writes: this.writeCount, errors: this.errorCount });
  }

  /** Stats for health checks */
  stats(): { writes: number; errors: number; active: boolean } {
    return {
      writes: this.writeCount,
      errors: this.errorCount,
      active: this.listeners.length > 0,
    };
  }

  private subscribe(): void {
    // Project lifecycle
    this.on("planning:project-created", (data) => {
      this.syncProject(data.projectId);
    });

    this.on("planning:project-resumed", (data) => {
      this.syncProject(data.projectId);
    });

    // Phase transitions — sync PROJECT.md (state changed) + phase-specific files
    this.on("planning:phase-started", (data) => {
      this.syncProject(data.projectId);
    });

    this.on("planning:phase-complete", (data) => {
      this.syncProject(data.projectId);
      this.syncPhaseArtifacts(data.projectId, data.phase);
    });

    // State transitions (from orchestration-core)
    this.on("planning:state-transition", (data) => {
      this.syncProject(data.projectId);
    });

    // Execution progress
    this.on("planning:execution-progress", (data) => {
      if (data.phaseId) {
        this.syncPhaseState(data.projectId, data.phaseId, {
          step: data.step ?? "unknown",
          wave: data.wave,
          status: data.status,
          timestamp: new Date().toISOString(),
        });
      }
    });

    // Verification
    this.on("planning:verification-complete", (data) => {
      this.syncProject(data.projectId);
      if (data.phaseId && data.result) {
        this.safeWrite("VERIFY.md", () => {
          writeVerifyFile(this.workspaceRoot!, data.projectId, data.phaseId, data.result);
        });
      }
    });

    // Project complete
    this.on("planning:complete", (data) => {
      this.syncProject(data.projectId);
      // Final full sync — all phase files
      this.fullSync(data.projectId);
    });

    // Requirement changes (from routes.ts PATCH/batch operations)
    this.on("planning:requirement-changed", (data) => {
      const reqs = this.store.listRequirements(data.projectId);
      if (reqs.length > 0) {
        this.safeWrite("REQUIREMENTS.md", () => {
          writeRequirementsFile(this.workspaceRoot!, data.projectId, reqs);
        });
      }
    });

    // Phase changes (from routes.ts PATCH operations)
    this.on("planning:phase-changed", (data) => {
      const phases = this.store.listPhases(data.projectId);
      if (phases.length > 0) {
        this.safeWrite("ROADMAP.md", () => {
          writeRoadmapFile(this.workspaceRoot!, data.projectId, phases);
        });
      }
    });

    // Discussion answers
    this.on("planning:discussion-answered", (data) => {
      if (data.phaseId) {
        const questions = this.store.listDiscussionQuestions(data.projectId, data.phaseId);
        if (questions.length > 0) {
          this.safeWrite(`DISCUSS.md(${data.phaseId})`, () => {
            writeDiscussFile(this.workspaceRoot!, data.projectId, data.phaseId, questions);
          });
        }
      }
    });
  }

  /** Subscribe to an event and track the listener for cleanup */
  private on(event: EventName, handler: (data: any) => void): void {
    const wrapped: EventHandler = (data) => {
      try {
        handler(data);
      } catch (error) {
        this.errorCount++;
        log.warn(`FileSyncDaemon error handling ${event}`, { error });
      }
    };
    eventBus.on(event, wrapped);
    this.listeners.push(() => eventBus.off(event, wrapped));
  }

  /** Write PROJECT.md with current state */
  private syncProject(projectId: string): void {
    if (!this.workspaceRoot) return;
    const project = this.store.getProject(projectId);
    if (!project) return;

    ensureProjectDir(this.workspaceRoot, projectId);
    this.safeWrite("PROJECT.md", () => {
      writeProjectFile(this.workspaceRoot!, project, project.projectContext);
    });
  }

  /** Write phase-specific artifacts based on what just completed */
  private syncPhaseArtifacts(projectId: string, phase: string): void {
    if (!this.workspaceRoot) return;

    switch (phase) {
      case "research": {
        const research = this.store.listResearch(projectId);
        if (research.length > 0) {
          this.safeWrite("RESEARCH.md", () => {
            writeResearchFile(this.workspaceRoot!, projectId, research);
          });
        }
        break;
      }
      case "requirements": {
        const reqs = this.store.listRequirements(projectId);
        if (reqs.length > 0) {
          this.safeWrite("REQUIREMENTS.md", () => {
            writeRequirementsFile(this.workspaceRoot!, projectId, reqs);
          });
        }
        break;
      }
      case "roadmap": {
        const phases = this.store.listPhases(projectId);
        if (phases.length > 0) {
          this.safeWrite("ROADMAP.md", () => {
            writeRoadmapFile(this.workspaceRoot!, projectId, phases);
          });
        }
        break;
      }
      case "discussing": {
        // Discussions are per-phase — sync all phase discussions
        const phases = this.store.listPhases(projectId);
        for (const p of phases) {
          const questions = this.store.listDiscussionQuestions(projectId, p.id);
          if (questions.length > 0) {
            this.safeWrite(`DISCUSS.md(${p.id})`, () => {
              writeDiscussFile(this.workspaceRoot!, projectId, p.id, questions);
            });
          }
        }
        break;
      }
    }
  }

  /** Write phase STATE.md with execution progress */
  private syncPhaseState(projectId: string, phaseId: string, state: Record<string, unknown>): void {
    if (!this.workspaceRoot) return;
    ensurePhaseDir(this.workspaceRoot, projectId, phaseId);
    this.safeWrite(`STATE.md(${phaseId})`, () => {
      writeStateFile(this.workspaceRoot!, projectId, phaseId, state);
    });
  }

  /** Full sync of all artifacts — used on project completion */
  private fullSync(projectId: string): void {
    if (!this.workspaceRoot) return;

    this.syncProject(projectId);

    const research = this.store.listResearch(projectId);
    if (research.length > 0) {
      this.safeWrite("RESEARCH.md", () => {
        writeResearchFile(this.workspaceRoot!, projectId, research);
      });
    }

    const reqs = this.store.listRequirements(projectId);
    if (reqs.length > 0) {
      this.safeWrite("REQUIREMENTS.md", () => {
        writeRequirementsFile(this.workspaceRoot!, projectId, reqs);
      });
    }

    const phases = this.store.listPhases(projectId);
    if (phases.length > 0) {
      this.safeWrite("ROADMAP.md", () => {
        writeRoadmapFile(this.workspaceRoot!, projectId, phases);
      });
    }

    for (const phase of phases) {
      const questions = this.store.listDiscussionQuestions(projectId, phase.id);
      if (questions.length > 0) {
        this.safeWrite(`DISCUSS.md(${phase.id})`, () => {
          writeDiscussFile(this.workspaceRoot!, projectId, phase.id, questions);
        });
      }
      if (phase.plan) {
        this.safeWrite(`PLAN.md(${phase.id})`, () => {
          writePlanFile(this.workspaceRoot!, projectId, phase.id, phase.plan);
        });
      }
      if (phase.verificationResult) {
        this.safeWrite(`VERIFY.md(${phase.id})`, () => {
          writeVerifyFile(this.workspaceRoot!, projectId, phase.id, phase.verificationResult as unknown as Record<string, unknown>);
        });
      }
    }

    log.info(`Full sync complete for project ${projectId}`);
  }

  /** Wrapper that catches file write errors without blocking */
  private safeWrite(label: string, fn: () => void): void {
    try {
      fn();
      this.writeCount++;
    } catch (error) {
      this.errorCount++;
      log.warn(`File write failed: ${label}`, { error });
    }
  }
}
