import { describe, expect, it } from "vitest";
import { transition, VALID_TRANSITIONS } from "./machine.js";
import { AletheiaError } from "../koina/errors.js";
import type { DianoiaState, PlanningEvent } from "./machine.js";

const ALL_EVENTS: PlanningEvent[] = [
  "START_QUESTIONING",
  "START_RESEARCH",
  "RESEARCH_COMPLETE",
  "REQUIREMENTS_COMPLETE",
  "ROADMAP_COMPLETE",
  "DISCUSSION_COMPLETE",
  "PLAN_READY",
  "VERIFY",
  "NEXT_PHASE",
  "ALL_PHASES_COMPLETE",
  "PHASE_FAILED",
  "BLOCK",
  "RESUME",
  "ABANDON",
];

function expectInvalidTransition(state: DianoiaState, event: PlanningEvent): void {
  expect(() => transition(state, event)).toThrow();
  try {
    transition(state, event);
  } catch (error) {
    expect((error as AletheiaError).code).toBe("PLANNING_INVALID_TRANSITION");
  }
}

describe("DianoiaFSM — valid transitions", () => {
  it("idle + START_QUESTIONING -> questioning", () => {
    expect(transition("idle", "START_QUESTIONING")).toBe("questioning");
  });

  it("idle + ABANDON -> abandoned", () => {
    expect(transition("idle", "ABANDON")).toBe("abandoned");
  });

  it("questioning + START_RESEARCH -> researching", () => {
    expect(transition("questioning", "START_RESEARCH")).toBe("researching");
  });

  it("questioning + ABANDON -> abandoned", () => {
    expect(transition("questioning", "ABANDON")).toBe("abandoned");
  });

  it("researching + RESEARCH_COMPLETE -> requirements", () => {
    expect(transition("researching", "RESEARCH_COMPLETE")).toBe("requirements");
  });

  it("researching + BLOCK -> blocked", () => {
    expect(transition("researching", "BLOCK")).toBe("blocked");
  });

  it("researching + ABANDON -> abandoned", () => {
    expect(transition("researching", "ABANDON")).toBe("abandoned");
  });

  it("requirements + REQUIREMENTS_COMPLETE -> roadmap", () => {
    expect(transition("requirements", "REQUIREMENTS_COMPLETE")).toBe("roadmap");
  });

  it("requirements + ABANDON -> abandoned", () => {
    expect(transition("requirements", "ABANDON")).toBe("abandoned");
  });

  it("roadmap + ROADMAP_COMPLETE -> discussing", () => {
    expect(transition("roadmap", "ROADMAP_COMPLETE")).toBe("discussing");
  });

  it("discussing + DISCUSSION_COMPLETE -> phase-planning", () => {
    expect(transition("discussing", "DISCUSSION_COMPLETE")).toBe("phase-planning");
  });

  it("discussing + ABANDON -> abandoned", () => {
    expect(transition("discussing", "ABANDON")).toBe("abandoned");
  });

  it("roadmap + ABANDON -> abandoned", () => {
    expect(transition("roadmap", "ABANDON")).toBe("abandoned");
  });

  it("phase-planning + PLAN_READY -> executing", () => {
    expect(transition("phase-planning", "PLAN_READY")).toBe("executing");
  });

  it("phase-planning + ABANDON -> abandoned", () => {
    expect(transition("phase-planning", "ABANDON")).toBe("abandoned");
  });

  it("executing + VERIFY -> verifying", () => {
    expect(transition("executing", "VERIFY")).toBe("verifying");
  });

  it("executing + BLOCK -> blocked", () => {
    expect(transition("executing", "BLOCK")).toBe("blocked");
  });

  it("executing + ABANDON -> abandoned", () => {
    expect(transition("executing", "ABANDON")).toBe("abandoned");
  });

  it("verifying + NEXT_PHASE -> discussing", () => {
    expect(transition("verifying", "NEXT_PHASE")).toBe("discussing");
  });

  it("verifying + ALL_PHASES_COMPLETE -> complete", () => {
    expect(transition("verifying", "ALL_PHASES_COMPLETE")).toBe("complete");
  });

  it("verifying + PHASE_FAILED -> blocked", () => {
    expect(transition("verifying", "PHASE_FAILED")).toBe("blocked");
  });

  it("verifying + ABANDON -> abandoned", () => {
    expect(transition("verifying", "ABANDON")).toBe("abandoned");
  });

  it("blocked + RESUME -> executing", () => {
    expect(transition("blocked", "RESUME")).toBe("executing");
  });

  it("blocked + ABANDON -> abandoned", () => {
    expect(transition("blocked", "ABANDON")).toBe("abandoned");
  });
});

describe("DianoiaFSM — invalid transitions", () => {
  it("idle + EXECUTE throws PLANNING_INVALID_TRANSITION", () => {
    expectInvalidTransition("idle", "VERIFY");
  });

  it("idle + VERIFY throws PLANNING_INVALID_TRANSITION", () => {
    expectInvalidTransition("idle", "VERIFY");
  });

  it("questioning + EXECUTE throws PLANNING_INVALID_TRANSITION", () => {
    expectInvalidTransition("questioning", "VERIFY");
  });

  it("executing + START_QUESTIONING throws PLANNING_INVALID_TRANSITION", () => {
    expectInvalidTransition("executing", "START_QUESTIONING");
  });

  it.each(ALL_EVENTS)("complete + %s throws PLANNING_INVALID_TRANSITION", (event) => {
    expectInvalidTransition("complete", event);
  });

  it.each(ALL_EVENTS)("abandoned + %s throws PLANNING_INVALID_TRANSITION", (event) => {
    expectInvalidTransition("abandoned", event);
  });
});

describe("DianoiaFSM — VALID_TRANSITIONS completeness", () => {
  it("covers all 12 states", () => {
    expect(Object.keys(VALID_TRANSITIONS).length).toBe(12);
  });

  it("complete state has no valid transitions", () => {
    expect(VALID_TRANSITIONS["complete"].length).toBe(0);
  });

  it("abandoned state has no valid transitions", () => {
    expect(VALID_TRANSITIONS["abandoned"].length).toBe(0);
  });

  it("all 12 DianoiaState values are present as keys", () => {
    const expected: DianoiaState[] = [
      "idle",
      "questioning",
      "researching",
      "requirements",
      "roadmap",
      "discussing",
      "phase-planning",
      "executing",
      "verifying",
      "complete",
      "blocked",
      "abandoned",
    ];
    for (const state of expected) {
      expect(VALID_TRANSITIONS).toHaveProperty(state);
    }
  });
});

describe("DianoiaFSM — state reachability (sequential scenarios)", () => {
  it("traces full happy path: idle -> complete", () => {
    let s: DianoiaState = "idle";
    s = transition(s, "START_QUESTIONING");
    expect(s).toBe("questioning");
    s = transition(s, "START_RESEARCH");
    expect(s).toBe("researching");
    s = transition(s, "RESEARCH_COMPLETE");
    expect(s).toBe("requirements");
    s = transition(s, "REQUIREMENTS_COMPLETE");
    expect(s).toBe("roadmap");
    s = transition(s, "ROADMAP_COMPLETE");
    expect(s).toBe("discussing");
    s = transition(s, "DISCUSSION_COMPLETE");
    expect(s).toBe("phase-planning");
    s = transition(s, "PLAN_READY");
    expect(s).toBe("executing");
    s = transition(s, "VERIFY");
    expect(s).toBe("verifying");
    s = transition(s, "ALL_PHASES_COMPLETE");
    expect(s).toBe("complete");
  });

  it("traces block and resume path: blocked -> executing", () => {
    let s: DianoiaState = "blocked";
    s = transition(s, "RESUME");
    expect(s).toBe("executing");
  });

  it("traces multi-phase loop: verifying -> discussing -> phase-planning (NEXT_PHASE)", () => {
    let s: DianoiaState = "verifying";
    s = transition(s, "NEXT_PHASE");
    expect(s).toBe("discussing");
    s = transition(s, "DISCUSSION_COMPLETE");
    expect(s).toBe("phase-planning");
    s = transition(s, "PLAN_READY");
    expect(s).toBe("executing");
    s = transition(s, "VERIFY");
    expect(s).toBe("verifying");
    s = transition(s, "ALL_PHASES_COMPLETE");
    expect(s).toBe("complete");
  });

  it("throws on first invalid step in sequence", () => {
    const s: DianoiaState = "idle";
    expect(() => transition(s, "VERIFY")).toThrow(AletheiaError);
  });
});
