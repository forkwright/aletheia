// plan_roadmap tool — manage roadmap generation and phase planning loop
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { RoadmapOrchestrator } from "./roadmap.js";
import type { PlanningConfig } from "./types.js";

const log = createLogger("dianoia:roadmap-tool");

export function createPlanRoadmapTool(
  orchestrator: DianoiaOrchestrator,
  roadmapOrchestrator: RoadmapOrchestrator,
): ToolHandler {
  return {
    definition: {
      name: "plan_roadmap",
      description:
        "Manage the roadmap generation and phase planning loop. Generate phases from requirements, adjust the roadmap interactively, commit when ready, then generate depth-calibrated plans for each phase.",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["generate", "adjust_phase", "commit", "plan_phases"],
            description: "Action to perform",
          },
          projectId: {
            type: "string",
            description: "Active planning project ID",
          },
          adjustment: {
            type: "string",
            description: "Freeform adjustment text (for adjust_phase action)",
          },
          phaseName: {
            type: "string",
            description: "Name of the phase to target (for adjust_phase action)",
          },
          requirements: {
            type: "array",
            description: "New requirements array to assign to the phase (for adjust_phase action)",
            items: { type: "string" },
          },
          newName: {
            type: "string",
            description: "New name for the phase (for adjust_phase action)",
          },
          newGoal: {
            type: "string",
            description: "New goal string for the phase (for adjust_phase action)",
          },
          phaseIds: {
            type: "array",
            description: "Array of phase IDs to plan (for plan_phases action; if omitted, plans all phases in phase_order sequence)",
            items: { type: "string" },
          },
        },
        required: ["action", "projectId"],
      },
    },
    execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      try {
        const action = input["action"] as string;
        const projectId = input["projectId"] as string;

        if (action === "generate") {
          const project = orchestrator.getProject(projectId);
          if (!project) {
            return Promise.resolve(JSON.stringify({ error: `Project not found: ${projectId}` }));
          }

          return roadmapOrchestrator
            .generateRoadmap(projectId, project.goal ?? "", context)
            .then((phases) => {
              const coverageResult = roadmapOrchestrator.validateCoverage(projectId, phases);
              if (!coverageResult.covered) {
                return JSON.stringify({
                  error: "Coverage validation failed",
                  missing: coverageResult.missing,
                });
              }

              const config = project.config as PlanningConfig;
              roadmapOrchestrator.commitRoadmap(projectId, phases);

              if (config.mode === "yolo") {
                orchestrator.completeRoadmap(projectId, context.nousId, context.sessionId);
                log.info(`Roadmap auto-committed (autonomous mode) for project ${projectId}`);
                return JSON.stringify({
                  committed: true,
                  phaseCount: phases.length,
                  message: "Roadmap auto-committed (autonomous mode).",
                });
              }

              const display = roadmapOrchestrator.formatRoadmapDisplay(
                roadmapOrchestrator.listPhases(projectId),
              );
              log.info(`Roadmap draft committed for project ${projectId}; ${phases.length} phases`);
              return JSON.stringify({
                draft: true,
                display,
                message: "Review the roadmap above. Adjust with adjust_phase action or say 'done' to commit.",
              });
            });
        }

        if (action === "adjust_phase") {
          const adjustment = input["adjustment"] as string;
          const phaseName = input["phaseName"] as string | undefined;
          const requirements = input["requirements"] as string[] | undefined;
          const newName = input["newName"] as string | undefined;
          const newGoal = input["newGoal"] as string | undefined;

          roadmapOrchestrator.adjustPhase(projectId, adjustment, {
            ...(phaseName !== undefined ? { phaseName } : {}),
            ...(requirements !== undefined ? { requirements } : {}),
            ...(newName !== undefined ? { newName } : {}),
            ...(newGoal !== undefined ? { newGoal } : {}),
          });

          const coverageResult = roadmapOrchestrator.validateCoverageFromDb(projectId);
          const display = roadmapOrchestrator.formatRoadmapDisplay(
            roadmapOrchestrator.listPhases(projectId),
          );

          log.info(`Phase adjusted for project ${projectId}; coverage=${coverageResult.covered}`);

          return Promise.resolve(
            JSON.stringify({
              adjusted: true,
              display,
              coverageGate: coverageResult.covered,
            }),
          );
        }

        if (action === "commit") {
          const coverageResult = roadmapOrchestrator.validateCoverageFromDb(projectId);
          if (!coverageResult.covered) {
            return Promise.resolve(
              JSON.stringify({
                error: "Coverage gate not met",
                missing: coverageResult.missing,
              }),
            );
          }

          orchestrator.completeRoadmap(projectId, context.nousId, context.sessionId);
          const phaseCount = orchestrator.listPhases(projectId).length;

          log.info(`Roadmap committed for project ${projectId}`);
          return Promise.resolve(
            JSON.stringify({
              committed: true,
              phaseCount,
              message: "Roadmap committed. Ready to generate phase plans.",
            }),
          );
        }

        if (action === "plan_phases") {
          const inputPhaseIds = input["phaseIds"] as string[] | undefined;

          const phases = orchestrator.listPhases(projectId);
          const orderedPhases = [...phases].sort((a, b) => a.phaseOrder - b.phaseOrder);
          const phaseIds = inputPhaseIds ?? orderedPhases.map((p) => p.id);

          const project = orchestrator.getProject(projectId);
          const config = (project?.config ?? {}) as { plan_check?: boolean; depth?: string };

          // Plan all phases in parallel to avoid sequential timeout
          return Promise.allSettled(
            phaseIds.map((phaseId) =>
              roadmapOrchestrator.planPhase(projectId, phaseId, config, context),
            ),
          ).then((results) => {
            const succeeded = results.filter((r) => r.status === "fulfilled").length;
            const failed = results
              .map((r, i) => (r.status === "rejected" ? { phaseId: phaseIds[i], error: String((r as PromiseRejectedResult).reason) } : null))
              .filter(Boolean);

            if (succeeded === phaseIds.length) {
              orchestrator.advanceToExecution(projectId, context.nousId, context.sessionId);
              log.info(`All ${phaseIds.length} phase plans generated for project ${projectId}`);
              return JSON.stringify({
                planned: phaseIds.length,
                message: "All phase plans generated. Ready for execution.",
              });
            }

            // Partial success — advance if at least one phase planned
            if (succeeded > 0) {
              orchestrator.advanceToExecution(projectId, context.nousId, context.sessionId);
              log.warn(`${succeeded}/${phaseIds.length} phase plans generated; ${failed.length} failed`, { failed });
              return JSON.stringify({
                planned: succeeded,
                failed: failed.length,
                failures: failed,
                message: `${succeeded} of ${phaseIds.length} phase plans generated. Failed phases can be retried.`,
              });
            }

            log.error(`All ${phaseIds.length} phase plans failed`, { failed });
            return JSON.stringify({
              error: "All phase plans failed",
              failures: failed,
            });
          });
        }

        return Promise.resolve(JSON.stringify({ error: `Unknown action: ${action}` }));
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log.error(`plan_roadmap failed: ${message}`);
        return Promise.resolve(JSON.stringify({ error: message }));
      }
    },
  };
}
