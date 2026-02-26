// plan_create tool — create a new Dianoia planning project with goal, description, and scope
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";

const log = createLogger("dianoia:create-tool");

export function createPlanCreateTool(orchestrator: DianoiaOrchestrator): ToolHandler {
  return {
    definition: {
      name: "plan_create",
      description:
        "Create a new Dianoia planning project. Returns the project ID and advances to the questioning/research phase.\n\n" +
        "USE WHEN:\n" +
        "- Starting a multi-phase planning effort\n" +
        "- User describes something to build and you want structured planning\n\n" +
        "After creation, use plan_research to spawn domain researchers (or skip), " +
        "then plan_requirements, plan_roadmap, plan_execute, and plan_verify.",
      input_schema: {
        type: "object",
        properties: {
          name: {
            type: "string",
            description: "Short project name",
          },
          description: {
            type: "string",
            description: "What we're building and why",
          },
          scope: {
            type: "string",
            description: "What's in scope and what's explicitly out of scope",
          },
          skipQuestioning: {
            type: "boolean",
            description: "Skip the questioning phase and go straight to researching (use when context is already clear)",
          },
        },
        required: ["name", "description"],
      },
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      try {
        const name = input["name"] as string;
        const description = input["description"] as string;
        const scope = (input["scope"] as string) ?? "";
        const skipQuestioning = (input["skipQuestioning"] as boolean) ?? false;

        // Check for existing active project
        const active = orchestrator.getActiveProject(context.nousId);
        if (active) {
          return JSON.stringify({
            error: `Active project already exists: "${active.goal || active.id}". Abandon it first or resume it.`,
            activeProjectId: active.id,
            state: active.state,
          });
        }

        // Create via orchestrator — this creates project in "idle" and transitions to "questioning"
        const message = orchestrator.handle(context.nousId, context.sessionId);

        // Now get the newly created project
        const project = orchestrator.getActiveProject(context.nousId);
        if (!project) {
          return JSON.stringify({ error: "Project creation failed — no active project found after handle()" });
        }

        // Set the goal and context from our inputs
        orchestrator.updateGoal(project.id, name);
        orchestrator.updateContext(project.id, {
          goal: name,
          constraints: scope ? [scope] : [],
          rawTranscript: [
            { turn: 1, text: `Project: ${name}` },
            { turn: 2, text: `Description: ${description}` },
            ...(scope ? [{ turn: 3, text: `Scope: ${scope}` }] : []),
          ],
        });

        // If skipQuestioning, advance straight to researching
        if (skipQuestioning) {
          orchestrator.confirmSynthesis(project.id, context.nousId, context.sessionId, {
            goal: name,
            constraints: scope ? [scope] : [],
          });
          log.info(`Created project ${project.id} and skipped to researching`);
          return JSON.stringify({
            projectId: project.id,
            state: "researching",
            message: `Project "${name}" created. Questioning skipped — ready for research phase. Use plan_research to proceed.`,
          });
        }

        log.info(`Created project ${project.id} in questioning state`);
        return JSON.stringify({
          projectId: project.id,
          state: "questioning",
          message: `Project "${name}" created. ${message}`,
        });
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        log.error(`plan_create failed: ${message}`);
        return JSON.stringify({ error: message });
      }
    },
  };
}
