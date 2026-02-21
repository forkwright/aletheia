// Structured planning with verification loops — SQLite-backed
import { createLogger } from "../../koina/logger.js";
import { generateId } from "../../koina/crypto.js";
import type { ToolContext, ToolHandler } from "../registry.js";
import type { ExecutionPlanStep, SessionStore } from "../../mneme/store.js";

const log = createLogger("organon.plan");

export function createPlanTools(store: SessionStore): ToolHandler[] {
  const planCreate: ToolHandler = {
    definition: {
      name: "plan_create",
      description:
        "Create a structured plan with steps, dependencies, and acceptance criteria.\n\n" +
        "USE WHEN:\n" +
        "- Task has 3+ steps that benefit from explicit tracking\n" +
        "- Steps have dependencies that determine execution order\n" +
        "- You want to track progress and verify acceptance criteria\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Task is simple and linear — just do it\n" +
        "- You're brainstorming, not executing\n\n" +
        "TIPS:\n" +
        "- Steps can have dependsOn arrays for dependency ordering\n" +
        "- Use plan_status to check progress and find actionable steps\n" +
        "- Use plan_step_complete/plan_step_fail to update progress\n" +
        "- Failed steps cascade-skip dependent steps automatically",
      input_schema: {
        type: "object",
        properties: {
          goal: {
            type: "string",
            description: "The overall goal this plan achieves",
          },
          steps: {
            type: "array",
            items: {
              type: "object",
              properties: {
                id: { type: "string", description: "Short step identifier (e.g., 'setup', 'impl', 'test')" },
                description: { type: "string", description: "What this step accomplishes" },
                acceptanceCriteria: { type: "string", description: "How to verify this step is complete" },
                dependsOn: {
                  type: "array",
                  items: { type: "string" },
                  description: "IDs of steps that must complete before this one",
                },
              },
              required: ["id", "description"],
            },
            description: "Ordered list of steps",
          },
        },
        required: ["goal", "steps"],
      },
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const goal = input["goal"] as string;
      const rawSteps = input["steps"] as Array<Record<string, unknown>>;

      const steps: ExecutionPlanStep[] = rawSteps.map((s) => ({
        id: s["id"] as string,
        description: s["description"] as string,
        status: "pending" as const,
        ...(s["acceptanceCriteria"] ? { acceptanceCriteria: s["acceptanceCriteria"] as string } : {}),
        dependsOn: (s["dependsOn"] as string[]) ?? [],
      }));

      const stepIds = new Set(steps.map((s) => s.id));
      for (const step of steps) {
        for (const dep of step.dependsOn) {
          if (!stepIds.has(dep)) {
            return JSON.stringify({ error: `Step "${step.id}" depends on unknown step "${dep}"` });
          }
        }
      }

      const planId = `plan_${generateId()}`;
      store.createExecutionPlan({
        id: planId,
        sessionId: context.sessionId,
        nousId: context.nousId,
        goal,
        steps,
      });

      log.info(`Plan created: ${planId} for ${context.nousId} — ${steps.length} steps`);

      const actionable = steps
        .filter((s) => s.dependsOn.length === 0)
        .map((s) => s.id);

      return JSON.stringify({
        planId,
        stepCount: steps.length,
        actionableNow: actionable,
      });
    },
  };

  const planStatus: ToolHandler = {
    definition: {
      name: "plan_status",
      description:
        "Get the current status of a plan — progress, blocked steps, and what's actionable next.\n\n" +
        "TIPS:\n" +
        "- Shows which steps are immediately actionable (all deps met)\n" +
        "- Shows blocked steps and what they're waiting on",
      input_schema: {
        type: "object",
        properties: {
          planId: { type: "string", description: "Plan ID to check" },
        },
        required: ["planId"],
      },
    },
    async execute(input: Record<string, unknown>): Promise<string> {
      const planId = input["planId"] as string;
      const plan = store.getPlan(planId);
      if (!plan) return JSON.stringify({ error: `Plan not found: ${planId}` });

      const steps = plan.steps as unknown as ExecutionPlanStep[];
      const completed = steps.filter((s) => s.status === "completed");
      const failed = steps.filter((s) => s.status === "failed");
      const pending = steps.filter((s) => s.status === "pending");
      const inProgress = steps.filter((s) => s.status === "in_progress");

      const completedIds = new Set(completed.map((s) => s.id));
      const actionable = pending.filter((s) =>
        s.dependsOn.every((dep) => completedIds.has(dep)),
      );

      return JSON.stringify({
        planId: plan.id,
        goal: plan.steps.length > 0 ? (steps[0] as ExecutionPlanStep).description : "",
        status: plan.status,
        progress: `${completed.length}/${steps.length} complete`,
        steps: steps.map((s) => ({
          id: s.id,
          status: s.status,
          description: s.description,
          result: s.result?.slice(0, 200),
          failureReason: s.failureReason,
        })),
        actionableNow: actionable.map((s) => s.id),
        blocked: pending.filter((s) => !actionable.includes(s)).map((s) => ({
          id: s.id,
          waitingOn: s.dependsOn.filter((dep) => !completedIds.has(dep)),
        })),
        summary: {
          completed: completed.length,
          failed: failed.length,
          inProgress: inProgress.length,
          pending: pending.length,
        },
      });
    },
  };

  const planStepComplete: ToolHandler = {
    definition: {
      name: "plan_step_complete",
      description: "Mark a plan step as completed. Returns newly actionable steps that were waiting on this one.",
      input_schema: {
        type: "object",
        properties: {
          planId: { type: "string", description: "Plan ID" },
          stepId: { type: "string", description: "Step ID to mark complete" },
          result: { type: "string", description: "Brief description of what was accomplished" },
        },
        required: ["planId", "stepId"],
      },
    },
    async execute(input: Record<string, unknown>): Promise<string> {
      const planId = input["planId"] as string;
      const stepId = input["stepId"] as string;
      const result = input["result"] as string | undefined;

      const plan = store.getPlan(planId);
      if (!plan) return JSON.stringify({ error: `Plan not found: ${planId}` });

      const steps = plan.steps as unknown as ExecutionPlanStep[];
      const step = steps.find((s) => s.id === stepId);
      if (!step) return JSON.stringify({ error: `Step not found: ${stepId}` });

      step.status = "completed";
      if (result) step.result = result;
      step.completedAt = new Date().toISOString();

      const allDone = steps.every((s) =>
        s.status === "completed" || s.status === "skipped",
      );

      store.updatePlanSteps(planId, steps);
      if (allDone) {
        store.updatePlanStatus(planId, "completed");
        log.info(`Plan completed: ${planId}`);
      }

      const completedIds = new Set(
        steps.filter((s) => s.status === "completed").map((s) => s.id),
      );
      const newlyActionable = steps.filter(
        (s) =>
          s.status === "pending" &&
          s.dependsOn.every((dep) => completedIds.has(dep)),
      );

      return JSON.stringify({
        stepId,
        status: "completed",
        planStatus: allDone ? "completed" : plan.status,
        nextSteps: newlyActionable.map((s) => s.id),
      });
    },
  };

  const planStepFail: ToolHandler = {
    definition: {
      name: "plan_step_fail",
      description: "Mark a plan step as failed. Dependent steps are auto-skipped. Set abandon=true to cancel the entire plan.",
      input_schema: {
        type: "object",
        properties: {
          planId: { type: "string", description: "Plan ID" },
          stepId: { type: "string", description: "Step ID that failed" },
          reason: { type: "string", description: "Why the step failed" },
          abandon: { type: "boolean", description: "If true, abandon the entire plan" },
        },
        required: ["planId", "stepId", "reason"],
      },
    },
    async execute(input: Record<string, unknown>): Promise<string> {
      const planId = input["planId"] as string;
      const stepId = input["stepId"] as string;
      const reason = input["reason"] as string;
      const abandon = input["abandon"] as boolean | undefined;

      const plan = store.getPlan(planId);
      if (!plan) return JSON.stringify({ error: `Plan not found: ${planId}` });

      const steps = plan.steps as unknown as ExecutionPlanStep[];
      const step = steps.find((s) => s.id === stepId);
      if (!step) return JSON.stringify({ error: `Step not found: ${stepId}` });

      step.status = "failed";
      step.failureReason = reason;
      step.completedAt = new Date().toISOString();

      let planStatus = plan.status;

      if (abandon) {
        planStatus = "abandoned";
        log.info(`Plan abandoned: ${planId} — step ${stepId} failed: ${reason}`);
      } else {
        const skippable = findDependents(steps, stepId);
        for (const s of skippable) {
          if (s.status === "pending") {
            s.status = "skipped";
            s.failureReason = `Skipped — depends on failed step "${stepId}"`;
          }
        }

        const remaining = steps.filter(
          (s) => s.status === "pending" || s.status === "in_progress",
        );
        if (remaining.length === 0) {
          planStatus = "failed";
          log.info(`Plan failed: ${planId} — no remaining steps`);
        }
      }

      store.updatePlanSteps(planId, steps);
      store.updatePlanStatus(planId, planStatus);

      return JSON.stringify({
        stepId,
        status: "failed",
        reason,
        planStatus,
        skipped: steps
          .filter((s) => s.status === "skipped")
          .map((s) => s.id),
      });
    },
  };

  return [planCreate, planStatus, planStepComplete, planStepFail];
}

function findDependents(steps: ExecutionPlanStep[], failedId: string): ExecutionPlanStep[] {
  const dependents: ExecutionPlanStep[] = [];
  const failedIds = new Set([failedId]);

  let changed = true;
  while (changed) {
    changed = false;
    for (const step of steps) {
      if (failedIds.has(step.id)) continue;
      if (step.dependsOn.some((dep) => failedIds.has(dep))) {
        failedIds.add(step.id);
        dependents.push(step);
        changed = true;
      }
    }
  }

  return dependents;
}
