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
      const projectId = input["projectId"] as string;
      const skip = input["skip"] as boolean | undefined;

      if (skip === true) {
        log.warn(`Research skipped for project ${projectId} — skipResearch() not yet wired (plan 04-02)`);
        return JSON.stringify({ status: "skipped", projectId });
      }

      const project = orchestrator.getProject(projectId);
      const projectGoal = project?.goal ?? "";

      log.info(`Starting research for project ${projectId}: "${projectGoal}"`);

      const { stored, partial, failed } = await researchOrchestrator.runResearch(
        projectId,
        projectGoal,
        context,
      );

      return JSON.stringify({ status: "complete", stored, partial, failed });
    },
  };
}
