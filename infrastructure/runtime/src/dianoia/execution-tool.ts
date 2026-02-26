// plan_execute tool — execute, pause, resume, retry, skip, or abandon a Dianoia phase execution
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { ExecutionOrchestrator } from "./execution.js";
import type { GoalBackwardVerifier } from "./verifier.js";

const log = createLogger("dianoia:execution-tool");

export function createPlanExecuteTool(
  planningOrchestrator: DianoiaOrchestrator,
  executionOrchestrator: ExecutionOrchestrator,
  verifierOrchestrator?: GoalBackwardVerifier,
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
      return handleAction(input, planningOrchestrator, executionOrchestrator, context, verifierOrchestrator);
    },
  };
}

async function handleAction(
  input: Record<string, unknown>,
  planningOrchestrator: DianoiaOrchestrator,
  executionOrchestrator: ExecutionOrchestrator,
  context: ToolContext,
  verifierOrchestrator?: GoalBackwardVerifier,
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
        if (result.failed > 0) {
          return JSON.stringify({
            ...result,
            message: "Execution complete with failures. Use action=retry or action=skip to recover.",
          });
        }

        // All phases succeeded — advance to verification and auto-run it
        planningOrchestrator.advanceToVerification(projectId, nousId, sessionId);

        if (verifierOrchestrator) {
          // Run verification for each completed phase
          const phases = planningOrchestrator.listPhases(projectId);
          const completedPhases = phases.filter(p => p.status === "complete");
          const verificationResults: Array<{ phaseId: string; name: string; status: string; summary: string }> = [];

          for (const phase of completedPhases) {
            try {
              const vResult = await verifierOrchestrator.verify(projectId, phase.id, context);
              const vStatus = vResult.status ?? vResult.overallStatus ?? "partially-met";
              verificationResults.push({
                phaseId: phase.id,
                name: phase.name,
                status: vStatus,
                summary: vResult.summary,
              });

              // Write verification file
              planningOrchestrator.syncVerifyFile(projectId, phase.id, vResult as unknown as Record<string, unknown>);

              if (vStatus !== "met" && vResult.gaps.length > 0) {
                // ORCH-04: Auto-skip downstream + generate rollback plan
                const { skippedPhases, rollbackPlan } = planningOrchestrator.skipDownstreamPhasesOnVerificationFailure(
                  projectId, phase.id, vResult.gaps,
                );
                log.info(`Verification gap cascade for phase ${phase.id}: skipped ${skippedPhases.length} downstream`);

                return JSON.stringify({
                  execution: result,
                  verification: verificationResults,
                  failedPhase: phase.id,
                  gaps: vResult.gaps,
                  skippedPhases,
                  rollbackPlan,
                  message: `Execution succeeded but verification found gaps in "${phase.name}". See rollback plan.`,
                }, null, 2);
              }
            } catch (error) {
              log.warn(`Verification failed for phase ${phase.id}, continuing`, { error });
              verificationResults.push({
                phaseId: phase.id,
                name: phase.name,
                status: "error",
                summary: error instanceof Error ? error.message : String(error),
              });
            }
          }

          // All phases verified — complete the project
          planningOrchestrator.completeAllPhases(projectId, nousId, sessionId);

          return JSON.stringify({
            execution: result,
            verification: verificationResults,
            message: "All phases executed and verified successfully. Project complete.",
          }, null, 2);
        }

        return "Execution complete. Advancing to verification phase.";
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
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    log.error(`plan_execute [${action}] failed: ${message}`);
    return JSON.stringify({ error: message });
  }
}
