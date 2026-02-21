// Plan tool tests — create, status, step complete, step fail (SQLite-backed)
import { beforeEach, describe, expect, it } from "vitest";
import { SessionStore } from "../../mneme/store.js";
import { createPlanTools } from "./plan.js";

describe("createPlanTools", () => {
  let store: SessionStore;
  let tools: ReturnType<typeof createPlanTools>;
  let sessionId: string;

  function makeCtx() {
    return { nousId: "syn", sessionId, workspace: "/tmp/test" };
  }

  beforeEach(() => {
    store = new SessionStore(":memory:");
    // Create a session so the plans FK constraint is satisfied
    const session = store.createSession("syn", "main");
    sessionId = session.id;
    tools = createPlanTools(store);
  });

  it("returns 4 tool handlers", () => {
    expect(tools).toHaveLength(4);
    expect(tools.map((t) => t.definition.name)).toEqual([
      "plan_create", "plan_status", "plan_step_complete", "plan_step_fail",
    ]);
  });

  it("plan_create creates a plan and returns actionable steps", async () => {
    const create = tools[0]!;
    const result = JSON.parse(await create.execute({
      goal: "Deploy feature",
      steps: [
        { id: "setup", description: "Set up env" },
        { id: "impl", description: "Implement", dependsOn: ["setup"] },
        { id: "test", description: "Run tests", dependsOn: ["impl"] },
      ],
    }, makeCtx()));

    expect(result.planId).toMatch(/^plan_/);
    expect(result.stepCount).toBe(3);
    expect(result.actionableNow).toEqual(["setup"]);
  });

  it("plan_create rejects invalid dependency references", async () => {
    const create = tools[0]!;
    const result = JSON.parse(await create.execute({
      goal: "Bad plan",
      steps: [{ id: "a", description: "Step A", dependsOn: ["nonexistent"] }],
    }, makeCtx()));

    expect(result.error).toContain("unknown step");
  });

  it("plan_status returns plan details", async () => {
    const [create, status] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "Test plan",
      steps: [
        { id: "a", description: "Step A" },
        { id: "b", description: "Step B", dependsOn: ["a"] },
      ],
    }, makeCtx()));

    const result = JSON.parse(await status!.execute({
      planId: created.planId,
    }, makeCtx()));

    expect(result.status).toBe("executing");
    expect(result.progress).toBe("0/2 complete");
    expect(result.actionableNow).toEqual(["a"]);
    expect(result.blocked).toHaveLength(1);
    expect(result.blocked[0].id).toBe("b");
  });

  it("plan_status returns error for unknown plan", async () => {
    const status = tools[1]!;
    const result = JSON.parse(await status.execute({
      planId: "plan_nonexistent",
    }, makeCtx()));

    expect(result.error).toContain("not found");
  });

  it("plan_step_complete marks step done and unlocks dependents", async () => {
    const [create, , complete] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "Sequential",
      steps: [
        { id: "first", description: "First step" },
        { id: "second", description: "Second step", dependsOn: ["first"] },
      ],
    }, makeCtx()));

    const completeResult = JSON.parse(await complete!.execute({
      planId: created.planId,
      stepId: "first",
      result: "Done with first",
    }, makeCtx()));

    expect(completeResult.status).toBe("completed");
    expect(completeResult.nextSteps).toEqual(["second"]);
    expect(completeResult.planStatus).toBe("executing");
  });

  it("plan_step_complete marks plan completed when all steps done", async () => {
    const [create, , complete] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "One step",
      steps: [{ id: "only", description: "Only step" }],
    }, makeCtx()));

    const result = JSON.parse(await complete!.execute({
      planId: created.planId,
      stepId: "only",
    }, makeCtx()));

    expect(result.planStatus).toBe("completed");
  });

  it("plan_step_complete returns error for unknown plan/step", async () => {
    const complete = tools[2]!;
    const r1 = JSON.parse(await complete.execute({
      planId: "plan_bad", stepId: "x",
    }, makeCtx()));
    expect(r1.error).toContain("Plan not found");

    const [create] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "Test", steps: [{ id: "a", description: "A" }],
    }, makeCtx()));

    const r2 = JSON.parse(await complete.execute({
      planId: created.planId, stepId: "nonexistent",
    }, makeCtx()));
    expect(r2.error).toContain("Step not found");
  });

  it("plan_step_fail marks step failed and skips dependents", async () => {
    const [create, , , fail] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "Chain",
      steps: [
        { id: "a", description: "A" },
        { id: "b", description: "B", dependsOn: ["a"] },
        { id: "c", description: "C", dependsOn: ["b"] },
      ],
    }, makeCtx()));

    const result = JSON.parse(await fail!.execute({
      planId: created.planId,
      stepId: "a",
      reason: "Broke",
    }, makeCtx()));

    expect(result.status).toBe("failed");
    expect(result.skipped).toContain("b");
    expect(result.skipped).toContain("c");
    expect(result.planStatus).toBe("failed");
  });

  it("plan_step_fail with abandon abandons the whole plan", async () => {
    const [create, , , fail] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "Abandon test",
      steps: [
        { id: "a", description: "A" },
        { id: "b", description: "B" },
      ],
    }, makeCtx()));

    const result = JSON.parse(await fail!.execute({
      planId: created.planId,
      stepId: "a",
      reason: "Give up",
      abandon: true,
    }, makeCtx()));

    expect(result.planStatus).toBe("abandoned");
  });

  it("plan_step_fail returns error for unknown plan/step", async () => {
    const fail = tools[3]!;
    const r1 = JSON.parse(await fail.execute({
      planId: "plan_bad", stepId: "x", reason: "test",
    }, makeCtx()));
    expect(r1.error).toContain("Plan not found");
  });

  it("plan_step_fail keeps plan executing if other steps remain", async () => {
    const [create, , , fail] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "Parallel",
      steps: [
        { id: "a", description: "A" },
        { id: "b", description: "B" },
      ],
    }, makeCtx()));

    const result = JSON.parse(await fail!.execute({
      planId: created.planId,
      stepId: "a",
      reason: "Failed A",
    }, makeCtx()));

    expect(result.planStatus).toBe("executing");
  });

  it("plan persists across getPlan calls", async () => {
    const [create] = tools;
    const created = JSON.parse(await create!.execute({
      goal: "Persistence test",
      steps: [{ id: "s1", description: "Step 1" }],
    }, makeCtx()));

    const plan = store.getPlan(created.planId);
    expect(plan).not.toBeNull();
    expect(plan!.nousId).toBe("syn");
    expect(plan!.sessionId).toBe(sessionId);
    expect(plan!.status).toBe("executing");
  });
});
