// plan_execute tool — execute, pause, resume, retry, skip, or abandon a Dianoia phase execution
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { ExecutionOrchestrator } from "./execution.js";

const log = createLogger("dianoia:execution-tool");

export function createPlanExecuteTool(
  planningOrchestrator: DianoiaOrchestrator,
  executionOrchestrator: ExecutionOrchestrator,
): ToolHandler {
  return {
    definition: {
      name: "plan_execute",
      description:
        "Execute, pause, resume, retry, skip, or abandon a Dianoia phase execution. Use action=start to begin wave-based parallel execution, action=status to check progress.",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["start", "pause", "resume", "retry", "skip", "abandon", "status"],
            description: "Action to perform",
          },
          projectId: {
            type: "string",
            description: "Active planning project ID",
          },
          phaseId: {
            type: "string",
            description: "Phase ID (required for start, resume, retry, skip)",
          },
          planId: {
            type: "string",
            description: "Specific plan ID within the phase (required for retry, skip)",
          },
          nousId: {
            type: "string",
            description: "Nous ID for event bus context (optional)",
          },
          sessionId: {
            type: "string",
            description: "Session ID for event bus context (optional)",
          },
        },
        required: ["action", "projectId"],
      },
    },
    execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      return handleAction(input, planningOrchestrator, executionOrchestrator, context);
    },
  };
}

async function handleAction(
  input: Record<string, unknown>,
  planningOrchestrator: DianoiaOrchestrator,
  executionOrchestrator: ExecutionOrchestrator,
  context: ToolContext,
): Promise<string> {
  const action = input["action"] as string;
  const projectId = input["projectId"] as string;
  const planId = input["planId"] as string | undefined;
  const nousId = (input["nousId"] as string | undefined) ?? context.nousId;
  const sessionId = (input["sessionId"] as string | undefined) ?? context.sessionId;

  try {
    switch (action) {
      case "start": {
        const result = await executionOrchestrator.executePhase(projectId, context);
        if (result.failed === 0) {
          return planningOrchestrator.advanceToVerification(projectId, nousId, sessionId);
        }
        return JSON.stringify({
          ...result,
          message: "Execution complete with failures. Use action=retry or action=skip to recover.",
        });
      }

      case "pause": {
        planningOrchestrator.pauseExecution(projectId);
        return "Execution will pause after the current wave completes.";
      }

      case "resume": {
        const msg = planningOrchestrator.resumeExecution(projectId, nousId, sessionId);
        await executionOrchestrator.executePhase(projectId, context);
        return msg;
      }

      case "retry": {
        if (!planId) throw new Error("planId required for retry");
        const snapshot = executionOrchestrator.getExecutionSnapshot(projectId);
        const planEntry = snapshot.plans.find((p) => p.phaseId === planId);
        if (!planEntry) throw new Error(`Plan ${planId} not found in execution snapshot`);
        log.info(`Retrying plan ${planId} for project ${projectId}`);
        await executionOrchestrator.executePhase(projectId, context);
        return `Retrying plan ${planId} from beginning.`;
      }

      case "skip": {
        if (!planId) throw new Error("planId required for skip");
        const snapshot = executionOrchestrator.getExecutionSnapshot(projectId);
        const planEntry = snapshot.plans.find((p) => p.phaseId === planId);
        if (!planEntry) throw new Error(`Plan ${planId} not found`);
        log.info(`Skipping plan ${planId} for project ${projectId}`);
        return `Plan ${planId} skipped. Partial commits left in place.`;
      }

      case "abandon": {
        planningOrchestrator.abandon(projectId);
        return "Phase execution abandoned.";
      }

      case "status": {
        const snapshot = executionOrchestrator.getExecutionSnapshot(projectId);
        return JSON.stringify(snapshot, null, 2);
      }

      default:
        return JSON.stringify({ error: `Unknown action: ${action}` });
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    log.error(`plan_execute [${action}] failed: ${message}`);
    return JSON.stringify({ error: message });
  }
}
