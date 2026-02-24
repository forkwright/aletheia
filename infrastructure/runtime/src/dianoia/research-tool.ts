// plan_research tool — spawn 4 parallel domain researchers via sessions_dispatch
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { ResearchOrchestrator } from "./researcher.js";

const log = createLogger("dianoia:research-tool");

export function createPlanResearchTool(
  orchestrator: DianoiaOrchestrator,
  researchOrchestrator: ResearchOrchestrator,
): ToolHandler {
  return {
    definition: {
      name: "plan_research",
      description:
        "Spawn 4 parallel domain researchers (stack, features, architecture, pitfalls) for the active planning project. Call after project context is confirmed (state: researching). Pass skip: true if the user already knows the domain well.",
      input_schema: {
        type: "object",
        properties: {
          projectId: { type: "string", description: "Active planning project ID" },
          skip: { type: "boolean", description: "Skip research entirely (user knows the domain)" },
        },
        required: ["projectId"],
      },
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      try {
        const projectId = (input as unknown as { projectId: string }).projectId;
        const skip = (input as unknown as { skip?: boolean }).skip;

        if (skip === true) {
          const message = orchestrator.skipResearch(projectId, context.nousId, context.sessionId);
          log.info(`Research skipped for project ${projectId}`);
          return JSON.stringify({ status: "skipped", message });
        }

        const project = orchestrator.getProject(projectId);
        const projectGoal = project?.goal ?? "";

        log.info(`Starting research for project ${projectId}: "${projectGoal}"`);

        const { stored, partial, failed } = await researchOrchestrator.runResearch(
          projectId,
          projectGoal,
          context,
        );

        researchOrchestrator.transitionToRequirements(projectId);

        let message: string;
        if (partial > 0) {
          message = `Research complete (${stored} dimensions, ${partial} timed out). Synthesized from ${stored} of ${stored + partial + failed} dimensions. Moving to requirements.`;
        } else if (failed > 0) {
          message = `Research complete (${stored} dimensions, ${failed} failed). Moving to requirements.`;
        } else {
          message = "Research complete across all 4 dimensions. Moving to requirements.";
        }

        return JSON.stringify({ status: "complete", stored, partial, failed, message });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log.error(`plan_research failed: ${message}`);
        return JSON.stringify({ error: message });
      }
    },
  };
}
