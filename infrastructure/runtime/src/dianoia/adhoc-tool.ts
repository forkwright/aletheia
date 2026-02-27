// Ad-hoc work mode (ENG-13) — nous-initiated work without full planning ceremony
//
// Same quality guardrails (atomic commits, state tracking, deviation rules) but
// skips requirements, roadmap, discussion phases. Creates a single-phase project
// that goes directly to execution. Human-auditable via Dianoia tab but not
// human-invoked.

import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import { PlanningStore } from "./store.js";
import type { PlanningConfig } from "./types.js";

const log = createLogger("dianoia:adhoc");

export interface AdhocWorkRequest {
  /** Short name for the work item */
  name: string;
  /** What needs to be done */
  description: string;
  /** Why this needs doing now */
  reason?: string;
  /** Nous initiating the work */
  nousId: string;
  /** Session context */
  sessionId: string;
}

export interface AdhocWorkResult {
  projectId: string;
  phaseId: string;
  state: "executing";
  message: string;
}

/**
 * Create an ad-hoc work item that skips the full planning ceremony.
 * Creates a project with a single phase and advances directly to execution state.
 */
export function createAdhocWork(
  store: PlanningStore,
  request: AdhocWorkRequest,
): AdhocWorkResult {
  const config: PlanningConfig = {
    depth: "quick",
    parallelization: false,
    research: false,
    plan_check: false,
    verifier: true,
    mode: "yolo",
    pause_between_phases: false,
  };

  // Create project — goes to 'executing' immediately
  const project = store.createProject({
    nousId: request.nousId,
    sessionId: request.sessionId,
    goal: request.description,
    config,
  });

  // Skip to executing state
  store.updateProjectState(project.id, "executing");

  // Create a single phase
  const phase = store.createPhase({
    projectId: project.id,
    name: request.name,
    goal: request.description,
    requirements: [],
    successCriteria: [
      "Work completed as described",
      "Atomic git commit with proper prefix",
      "No regressions introduced",
    ],
    phaseOrder: 0,
    dependencies: [],
  });

  // Mark phase as executing
  store.updatePhaseStatus(phase.id, "executing");

  // Log as decision for audit trail
  store.logDecision({
    projectId: project.id,
    phaseId: phase.id,
    source: "agent",
    type: "adhoc-initiated",
    summary: `Ad-hoc work initiated: ${request.name}`,
    rationale: request.reason ?? "Agent-initiated work",
    context: {
      nousId: request.nousId,
      sessionId: request.sessionId,
      mode: "adhoc",
    },
  });

  eventBus.emit("planning:project-created", {
    projectId: project.id,
    nousId: request.nousId,
    mode: "adhoc",
  });

  log.info(`Ad-hoc work created: ${request.name}`, {
    projectId: project.id,
    phaseId: phase.id,
    nousId: request.nousId,
  });

  return {
    projectId: project.id,
    phaseId: phase.id,
    state: "executing",
    message: `Ad-hoc work "${request.name}" created and executing. Project ${project.id}, phase ${phase.id}. Tracked in Dianoia tab.`,
  };
}

/**
 * Tool definition for plan_adhoc — exposed to agents.
 */
export const ADHOC_TOOL_DEFINITION = {
  name: "plan_adhoc",
  description:
    "Start ad-hoc work without full planning ceremony. Creates a tracked project that goes straight to execution. " +
    "Same quality guardrails (commits, verification) but skips requirements/roadmap/discussion. " +
    "Use for small-to-medium tasks that don't need decomposition.",
  input_schema: {
    type: "object" as const,
    required: ["name", "description"],
    properties: {
      name: {
        type: "string",
        description: "Short name for the work (e.g., 'fix login redirect', 'add dark mode toggle')",
      },
      description: {
        type: "string",
        description: "What needs to be done — specific enough to execute without further decomposition",
      },
      reason: {
        type: "string",
        description: "Why this needs doing now (optional, for audit trail)",
      },
    },
  },
};
