// Structured planning with verification loops
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { createLogger } from "../../koina/logger.js";
import type { ToolHandler, ToolContext } from "../registry.js";

const log = createLogger("organon.plan");

interface PlanStep {
  id: string;
  description: string;
  status: "pending" | "in_progress" | "completed" | "failed" | "skipped";
  acceptanceCriteria?: string;
  dependsOn: string[];
  result?: string;
  failureReason?: string;
  startedAt?: string;
  completedAt?: string;
}

interface Plan {
  id: string;
  nousId: string;
  sessionId: string;
  goal: string;
  status: "planning" | "executing" | "completed" | "failed" | "abandoned";
  steps: PlanStep[];
  createdAt: string;
  updatedAt: string;
  completedAt?: string;
  summary?: string;
}

function plansFilePath(workspace: string): string {
  return join(workspace, "..", "..", "shared", "blackboard", "plans.jsonl");
}

function loadPlans(filePath: string): Plan[] {
  if (!existsSync(filePath)) return [];
  const content = readFileSync(filePath, "utf-8").trim();
  if (!content) return [];
  return content.split("\n").map((line) => JSON.parse(line) as Plan);
}

function savePlans(filePath: string, plans: Plan[]): void {
  const dir = dirname(filePath);
  if (!existsSync(dir)) mkdirSync(dir, { recursive: true });
  writeFileSync(filePath, plans.map((p) => JSON.stringify(p)).join("\n") + "\n");
}

function generatePlanId(): string {
  return `plan_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 6)}`;
}

export function createPlanTools(): ToolHandler[] {
  const planCreate: ToolHandler = {
    definition: {
      name: "plan_create",
      description:
        "Create a structured plan with steps, dependencies, and acceptance criteria. " +
        "Use for multi-step tasks that benefit from explicit decomposition and tracking.",
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

      const steps: PlanStep[] = rawSteps.map((s) => ({
        id: s["id"] as string,
        description: s["description"] as string,
        status: "pending",
        acceptanceCriteria: s["acceptanceCriteria"] as string | undefined,
        dependsOn: (s["dependsOn"] as string[]) ?? [],
      }));

      // Validate dependencies reference valid step IDs
      const stepIds = new Set(steps.map((s) => s.id));
      for (const step of steps) {
        for (const dep of step.dependsOn) {
          if (!stepIds.has(dep)) {
            return JSON.stringify({ error: `Step "${step.id}" depends on unknown step "${dep}"` });
          }
        }
      }

      const plan: Plan = {
        id: generatePlanId(),
        nousId: context.nousId,
        sessionId: context.sessionId,
        goal,
        status: "executing",
        steps,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      };

      const filePath = plansFilePath(context.workspace);
      const plans = loadPlans(filePath);
      plans.push(plan);
      savePlans(filePath, plans);

      log.info(`Plan created: ${plan.id} for ${context.nousId} — ${steps.length} steps`);

      // Return which steps are immediately actionable (no dependencies)
      const actionable = steps
        .filter((s) => s.dependsOn.length === 0)
        .map((s) => s.id);

      return JSON.stringify({
        planId: plan.id,
        stepCount: steps.length,
        actionableNow: actionable,
      });
    },
  };

  const planStatus: ToolHandler = {
    definition: {
      name: "plan_status",
      description: "Get the current status of a plan and its steps. Shows which steps are actionable next.",
      input_schema: {
        type: "object",
        properties: {
          planId: { type: "string", description: "Plan ID to check" },
        },
        required: ["planId"],
      },
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const planId = input["planId"] as string;
      const filePath = plansFilePath(context.workspace);
      const plans = loadPlans(filePath);
      const plan = plans.find((p) => p.id === planId);

      if (!plan) {
        return JSON.stringify({ error: `Plan not found: ${planId}` });
      }

      const completed = plan.steps.filter((s) => s.status === "completed");
      const failed = plan.steps.filter((s) => s.status === "failed");
      const pending = plan.steps.filter((s) => s.status === "pending");
      const inProgress = plan.steps.filter((s) => s.status === "in_progress");

      // Find actionable steps: pending with all dependencies completed
      const completedIds = new Set(completed.map((s) => s.id));
      const actionable = pending.filter((s) =>
        s.dependsOn.every((dep) => completedIds.has(dep)),
      );

      return JSON.stringify({
        planId: plan.id,
        goal: plan.goal,
        status: plan.status,
        progress: `${completed.length}/${plan.steps.length} complete`,
        steps: plan.steps.map((s) => ({
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
      description: "Mark a plan step as completed with an optional result summary.",
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
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const planId = input["planId"] as string;
      const stepId = input["stepId"] as string;
      const result = input["result"] as string | undefined;

      const filePath = plansFilePath(context.workspace);
      const plans = loadPlans(filePath);
      const plan = plans.find((p) => p.id === planId);
      if (!plan) return JSON.stringify({ error: `Plan not found: ${planId}` });

      const step = plan.steps.find((s) => s.id === stepId);
      if (!step) return JSON.stringify({ error: `Step not found: ${stepId}` });

      step.status = "completed";
      step.result = result;
      step.completedAt = new Date().toISOString();
      plan.updatedAt = new Date().toISOString();

      // Check if all steps are done
      const allDone = plan.steps.every((s) =>
        s.status === "completed" || s.status === "skipped",
      );
      if (allDone) {
        plan.status = "completed";
        plan.completedAt = new Date().toISOString();
        log.info(`Plan completed: ${planId}`);
      }

      savePlans(filePath, plans);

      // Find newly actionable steps
      const completedIds = new Set(
        plan.steps.filter((s) => s.status === "completed").map((s) => s.id),
      );
      const newlyActionable = plan.steps.filter(
        (s) =>
          s.status === "pending" &&
          s.dependsOn.every((dep) => completedIds.has(dep)),
      );

      return JSON.stringify({
        stepId,
        status: "completed",
        planStatus: plan.status,
        nextSteps: newlyActionable.map((s) => s.id),
      });
    },
  };

  const planStepFail: ToolHandler = {
    definition: {
      name: "plan_step_fail",
      description: "Mark a plan step as failed with a reason. The plan continues — other non-dependent steps can proceed.",
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
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const planId = input["planId"] as string;
      const stepId = input["stepId"] as string;
      const reason = input["reason"] as string;
      const abandon = input["abandon"] as boolean | undefined;

      const filePath = plansFilePath(context.workspace);
      const plans = loadPlans(filePath);
      const plan = plans.find((p) => p.id === planId);
      if (!plan) return JSON.stringify({ error: `Plan not found: ${planId}` });

      const step = plan.steps.find((s) => s.id === stepId);
      if (!step) return JSON.stringify({ error: `Step not found: ${stepId}` });

      step.status = "failed";
      step.failureReason = reason;
      step.completedAt = new Date().toISOString();
      plan.updatedAt = new Date().toISOString();

      if (abandon) {
        plan.status = "abandoned";
        plan.completedAt = new Date().toISOString();
        log.info(`Plan abandoned: ${planId} — step ${stepId} failed: ${reason}`);
      } else {
        // Skip steps that depend on the failed step (cascade)
        const skippable = findDependents(plan.steps, stepId);
        for (const s of skippable) {
          if (s.status === "pending") {
            s.status = "skipped";
            s.failureReason = `Skipped — depends on failed step "${stepId}"`;
          }
        }

        // Check if any steps can still proceed
        const remaining = plan.steps.filter(
          (s) => s.status === "pending" || s.status === "in_progress",
        );
        if (remaining.length === 0) {
          plan.status = "failed";
          plan.completedAt = new Date().toISOString();
          log.info(`Plan failed: ${planId} — no remaining steps`);
        }
      }

      savePlans(filePath, plans);

      return JSON.stringify({
        stepId,
        status: "failed",
        reason,
        planStatus: plan.status,
        skipped: plan.steps
          .filter((s) => s.status === "skipped")
          .map((s) => s.id),
      });
    },
  };

  return [planCreate, planStatus, planStepComplete, planStepFail];
}

function findDependents(steps: PlanStep[], failedId: string): PlanStep[] {
  const dependents: PlanStep[] = [];
  const failedIds = new Set([failedId]);

  // Cascade: find all steps that transitively depend on the failed step
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
