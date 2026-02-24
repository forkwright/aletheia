// plan_verify tool — run goal-backward verification, override, status, and checkpoint management
import { createLogger } from "../koina/logger.js";
import { eventBus } from "../koina/event-bus.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { GoalBackwardVerifier } from "./verifier.js";
import type { CheckpointSystem } from "./checkpoint.js";
import type { PlanningStore } from "./store.js";

const log = createLogger("dianoia:verifier-tool");

export function createPlanVerifyTool(
  planningOrchestrator: DianoiaOrchestrator,
  verifierOrchestrator: GoalBackwardVerifier,
  checkpointSystem: CheckpointSystem,
  store: PlanningStore,
): ToolHandler {
  return {
    definition: {
      name: "plan_verify",
      description:
        "Run goal-backward verification on a completed phase, manage gap closure, and handle checkpoint approvals.",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["run", "override", "status", "approve_checkpoint", "skip_checkpoint"],
            description: "Action to perform",
          },
          projectId: {
            type: "string",
            description: "Active planning project ID",
          },
          phaseId: {
            type: "string",
            description: "Phase to verify — required for run/override/status",
          },
          checkpointId: {
            type: "string",
            description: "Checkpoint to approve or skip",
          },
          overrideNote: {
            type: "string",
            description: "Required note when action=override",
          },
          userNote: {
            type: "string",
            description: "Optional note captured with checkpoint decision",
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
      return handleVerifyAction(input, planningOrchestrator, verifierOrchestrator, checkpointSystem, store, context);
    },
  };
}

async function handleVerifyAction(
  input: Record<string, unknown>,
  planningOrchestrator: DianoiaOrchestrator,
  verifierOrchestrator: GoalBackwardVerifier,
  checkpointSystem: CheckpointSystem,
  store: PlanningStore,
  context: ToolContext,
): Promise<string> {
  const action = input["action"] as string;
  const projectId = input["projectId"] as string;
  const nousId = (input["nousId"] as string | undefined) ?? context.nousId;
  const sessionId = (input["sessionId"] as string | undefined) ?? context.sessionId;

  try {
    switch (action) {
      case "run": {
        const phaseId = input["phaseId"] as string | undefined;
        if (!phaseId) throw new Error("phaseId required for action=run");

        const result = await verifierOrchestrator.verify(projectId, phaseId, context);

        if (result.status === "met") {
          const msg = planningOrchestrator.advanceToNextPhase(projectId, nousId, sessionId);
          return `${msg}\n\nVerification summary: ${result.summary}`;
        }

        planningOrchestrator.blockOnVerificationFailure(projectId);

        const gapPlans = verifierOrchestrator.generateGapPlans(phaseId, result.gaps);
        const gapPlansSummary = gapPlans.map((p) => (p as { name?: string }).name ?? "unnamed fix").join(", ");

        return JSON.stringify({
          status: result.status,
          summary: result.summary,
          gaps: result.gaps,
          gapPlansSummary: gapPlansSummary || "no gap plans generated",
          options: [
            { action: "fix_now", description: "Address the gaps listed above, then re-run verification." },
            { action: "override", description: "Override the failure with a written justification (action=override)." },
            { action: "abandon", description: "Abandon this planning project." },
          ],
        }, null, 2);
      }

      case "override": {
        const phaseId = input["phaseId"] as string | undefined;
        const overrideNote = input["overrideNote"] as string | undefined;
        if (!phaseId) throw new Error("phaseId required for action=override");
        if (!overrideNote) throw new Error("overrideNote required for action=override");

        const phase = store.getPhaseOrThrow(phaseId);
        const existing = phase.verificationResult;
        const updatedResult = existing
          ? { ...existing, overridden: true as const, overrideNote }
          : {
              status: "partially-met" as const,
              summary: "Manually overridden.",
              gaps: [],
              verifiedAt: new Date().toISOString(),
              overridden: true as const,
              overrideNote,
            };

        store.updatePhaseVerificationResult(phaseId, updatedResult);

        await checkpointSystem.evaluate({
          projectId,
          riskLevel: "medium",
          type: "verification-override",
          question: `User overriding verification failure: ${overrideNote}`,
          context: { phaseId, overrideNote },
          nousId,
          sessionId,
        });

        planningOrchestrator.advanceToNextPhase(projectId, nousId, sessionId);
        log.info(`Verification override recorded for phase ${phaseId}`, { projectId, overrideNote });
        return "Verification override recorded. Advancing to next phase.";
      }

      case "status": {
        const phaseId = input["phaseId"] as string | undefined;
        if (!phaseId) throw new Error("phaseId required for action=status");

        const phase = store.getPhaseOrThrow(phaseId);
        if (!phase.verificationResult) {
          return "No verification result found for this phase.";
        }
        return JSON.stringify(phase.verificationResult, null, 2);
      }

      case "approve_checkpoint": {
        const checkpointId = input["checkpointId"] as string | undefined;
        if (!checkpointId) throw new Error("checkpointId required for action=approve_checkpoint");

        const userNote = input["userNote"] as string | undefined;
        store.resolveCheckpoint(checkpointId, "approved", {
          autoApproved: false,
          ...(userNote ? { userNote } : {}),
        });
        eventBus.emit("planning:checkpoint", {
          checkpointId,
          decision: "approved",
          autoApproved: false,
          userNote: userNote ?? null,
        });
        return "Checkpoint approved.";
      }

      case "skip_checkpoint": {
        const checkpointId = input["checkpointId"] as string | undefined;
        if (!checkpointId) throw new Error("checkpointId required for action=skip_checkpoint");

        const userNote = input["userNote"] as string | undefined;
        store.resolveCheckpoint(checkpointId, "skipped", {
          autoApproved: false,
          ...(userNote ? { userNote } : {}),
        });
        eventBus.emit("planning:checkpoint", {
          checkpointId,
          decision: "skipped",
          autoApproved: false,
          userNote: userNote ?? null,
        });
        return "Checkpoint skipped.";
      }

      default:
        return JSON.stringify({ error: `Unknown action: ${action}` });
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    log.error(`plan_verify [${action}] failed: ${message}`);
    return JSON.stringify({ error: message });
  }
}
