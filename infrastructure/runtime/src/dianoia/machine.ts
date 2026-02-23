import { AletheiaError } from "../koina/errors.js";
import type { DianoiaState } from "./types.js";

export type { DianoiaState };

export type PlanningEvent =
  | "START_QUESTIONING"
  | "START_RESEARCH"
  | "RESEARCH_COMPLETE"
  | "REQUIREMENTS_COMPLETE"
  | "ROADMAP_COMPLETE"
  | "PLAN_READY"
  | "VERIFY"
  | "NEXT_PHASE"
  | "ALL_PHASES_COMPLETE"
  | "PHASE_FAILED"
  | "BLOCK"
  | "RESUME"
  | "ABANDON";

export const VALID_TRANSITIONS: Record<DianoiaState, ReadonlyArray<PlanningEvent>> = {
  idle: ["START_QUESTIONING", "ABANDON"],
  questioning: ["START_RESEARCH", "ABANDON"],
  researching: ["RESEARCH_COMPLETE", "BLOCK", "ABANDON"],
  requirements: ["REQUIREMENTS_COMPLETE", "ABANDON"],
  roadmap: ["ROADMAP_COMPLETE", "ABANDON"],
  "phase-planning": ["PLAN_READY", "ABANDON"],
  executing: ["VERIFY", "BLOCK", "ABANDON"],
  verifying: ["NEXT_PHASE", "ALL_PHASES_COMPLETE", "PHASE_FAILED", "ABANDON"],
  blocked: ["RESUME", "ABANDON"],
  complete: [],
  abandoned: [],
};

const TRANSITION_RESULT: Partial<Record<DianoiaState, Partial<Record<PlanningEvent, DianoiaState>>>> = {
  idle: { START_QUESTIONING: "questioning", ABANDON: "abandoned" },
  questioning: { START_RESEARCH: "researching", ABANDON: "abandoned" },
  researching: { RESEARCH_COMPLETE: "requirements", BLOCK: "blocked", ABANDON: "abandoned" },
  requirements: { REQUIREMENTS_COMPLETE: "roadmap", ABANDON: "abandoned" },
  roadmap: { ROADMAP_COMPLETE: "phase-planning", ABANDON: "abandoned" },
  "phase-planning": { PLAN_READY: "executing", ABANDON: "abandoned" },
  executing: { VERIFY: "verifying", BLOCK: "blocked", ABANDON: "abandoned" },
  verifying: {
    NEXT_PHASE: "phase-planning",
    ALL_PHASES_COMPLETE: "complete",
    PHASE_FAILED: "blocked",
    ABANDON: "abandoned",
  },
  blocked: { RESUME: "executing", ABANDON: "abandoned" },
};

export function transition(state: DianoiaState, event: PlanningEvent): DianoiaState {
  const allowed = VALID_TRANSITIONS[state] ?? [];
  if (!(allowed as ReadonlyArray<string>).includes(event)) {
    throw new AletheiaError({
      code: "PLANNING_INVALID_TRANSITION",
      module: "dianoia",
      message: `Invalid transition: ${state} + ${event}`,
      context: { state, event, allowed },
    });
  }
  const next = TRANSITION_RESULT[state]?.[event];
  if (next === undefined) {
    throw new AletheiaError({
      code: "PLANNING_STATE_CORRUPT",
      module: "dianoia",
      message: `Transition table incomplete: ${state} + ${event}`,
      context: { state, event },
    });
  }
  return next;
}
