// Plan proposal tool — proposes a multi-step plan for human approval
// When called, stores the plan and signals the turn to yield plan_proposed
import { createLogger } from "../../koina/logger.js";
import { generateId } from "../../koina/crypto.js";
import type { ToolContext, ToolHandler } from "../registry.js";

const log = createLogger("organon.plan-propose");

/** Cost per million tokens by model tier. */
const MODEL_COSTS: Record<string, { input: number; output: number }> = {
  opus:     { input: 5.00,  output: 25.00 },
  sonnet:   { input: 3.00,  output: 15.00 },
  haiku:    { input: 1.00,  output: 5.00  },
};

/** Estimate cost in cents for a step based on role. */
function estimateStepCost(role: string): number {
  const model = role === "self" ? "opus"
    : role === "explorer" || role === "runner" ? "haiku"
    : "sonnet"; // coder, reviewer, researcher
  const costs = MODEL_COSTS[model] ?? MODEL_COSTS["sonnet"]!;
  // Rough estimate: 15K input tokens + 3K output tokens per step
  const inputCost = (15_000 / 1_000_000) * costs.input;
  const outputCost = (3_000 / 1_000_000) * costs.output;
  return Math.round((inputCost + outputCost) * 100); // cents
}

export const PLAN_PROPOSED_MARKER = "__PLAN_PROPOSED__";

export function createPlanProposeHandler(): ToolHandler {
  return {
    definition: {
      name: "plan_propose",
      description:
        "Propose a multi-step execution plan for human approval before proceeding. " +
        "The turn will pause and the human sees the plan with cost estimates, " +
        "checkboxes per step, and Approve/Edit/Cancel buttons.\n\n" +
        "USE WHEN:\n" +
        "- Task has 4+ steps spanning different roles (code, research, review)\n" +
        "- Estimated cost exceeds $0.50 and you want explicit approval\n" +
        "- Steps could be run in parallel and you want the human to see the structure\n" +
        "- Complex multi-agent coordination where the human should review the plan\n\n" +
        "DO NOT USE WHEN:\n" +
        "- Task is simple (≤3 steps) — just do it\n" +
        "- You're already executing and don't need approval\n" +
        "- The human said 'just do it' or similar",
      input_schema: {
        type: "object" as const,
        properties: {
          goal: {
            type: "string",
            description: "What this plan accomplishes — shown as the plan title",
          },
          steps: {
            type: "array",
            items: {
              type: "object",
              properties: {
                label: { type: "string", description: "Human-readable description of this step" },
                role: {
                  type: "string",
                  enum: ["coder", "reviewer", "researcher", "explorer", "runner", "self"],
                  description: "Who executes: self (you), or a sub-agent role",
                },
                parallel: {
                  type: "array",
                  items: { type: "number" },
                  description: "Step indices (0-based) this step can run alongside",
                },
              },
              required: ["label", "role"],
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

      if (!rawSteps?.length) {
        return JSON.stringify({ error: "Plan must have at least one step" });
      }

      const steps = rawSteps.map((s, i) => ({
        id: i,
        label: s["label"] as string,
        role: (s["role"] as string) ?? "self",
        estimatedCostCents: estimateStepCost((s["role"] as string) ?? "self"),
        parallel: (s["parallel"] as number[]) ?? undefined,
        status: "pending" as const,
      }));

      const totalCost = steps.reduce((sum, s) => sum + s.estimatedCostCents, 0);
      const planId = `plan_${generateId().slice(0, 12)}`;

      log.info(`Plan proposed: ${planId} — ${steps.length} steps, ~$${(totalCost / 100).toFixed(2)} for ${context.nousId}`);

      // Store plan data in context metadata for the execute stage to pick up
      // The execute stage reads this marker and yields plan_proposed
      return JSON.stringify({
        __marker: PLAN_PROPOSED_MARKER,
        plan: {
          id: planId,
          sessionId: context.sessionId,
          nousId: context.nousId,
          goal,
          steps,
          totalEstimatedCostCents: totalCost,
          status: "awaiting_approval",
          createdAt: new Date().toISOString(),
        },
      });
    },
  };
}
