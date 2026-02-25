// plan_requirements tool — manage requirements scoping loop: present, persist, update, check, complete
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import type { CategoryProposal, RequirementsOrchestrator, ScopingDecision } from "./requirements.js";

const log = createLogger("dianoia:requirements-tool");

export function createPlanRequirementsTool(
  orchestrator: DianoiaOrchestrator,
  requirementsOrchestrator: RequirementsOrchestrator,
): ToolHandler {
  return {
    definition: {
      name: "plan_requirements",
      description:
        "Manage the requirements scoping loop. Present feature categories to the user one at a time with v1/v2/out-of-scope proposals, persist confirmed decisions, handle freeform adjustments, check coverage gate, and advance to roadmap phase when complete.",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["present_category", "persist_category", "update_requirement", "check_coverage", "complete"],
            description: "Action to perform",
          },
          projectId: {
            type: "string",
            description: "Active planning project ID",
          },
          category: {
            type: "object",
            description: "Category proposal (for persist_category action)",
            properties: {
              category: { type: "string" },
              categoryName: { type: "string" },
              tableStakes: {
                type: "array",
                items: {
                  type: "object",
                  properties: {
                    name: { type: "string" },
                    description: { type: "string" },
                    isTableStakes: { type: "boolean" },
                    proposedTier: { type: "string", enum: ["v1", "v2", "out-of-scope"] },
                    proposedRationale: { type: "string" },
                  },
                  required: ["name", "description", "isTableStakes", "proposedTier"],
                },
              },
              differentiators: {
                type: "array",
                items: {
                  type: "object",
                  properties: {
                    name: { type: "string" },
                    description: { type: "string" },
                    isTableStakes: { type: "boolean" },
                    proposedTier: { type: "string", enum: ["v1", "v2", "out-of-scope"] },
                    proposedRationale: { type: "string" },
                  },
                  required: ["name", "description", "isTableStakes", "proposedTier"],
                },
              },
            },
            required: ["category", "categoryName", "tableStakes", "differentiators"],
          },
          decisions: {
            type: "array",
            description: "Scoping decisions for persist_category action",
            items: {
              type: "object",
              properties: {
                name: { type: "string" },
                tier: { type: "string", enum: ["v1", "v2", "out-of-scope"] },
                rationale: { type: "string" },
              },
              required: ["name", "tier"],
            },
          },
          reqId: {
            type: "string",
            description: "Requirement ID to update (e.g. AUTH-01), for update_requirement action",
          },
          updates: {
            type: "object",
            description: "Fields to update for update_requirement action",
            properties: {
              tier: { type: "string", enum: ["v1", "v2", "out-of-scope"] },
              rationale: { type: "string" },
            },
          },
          presentedCategories: {
            type: "array",
            description: "List of category codes presented so far (for check_coverage and complete actions)",
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

        if (action === "present_category") {
          const synthesis = requirementsOrchestrator.getSynthesis(projectId);
          if (!synthesis) {
            return Promise.resolve(JSON.stringify({
              synthesis: null,
              message:
                "No research synthesis available (research was skipped). Derive feature categories directly from the project goal and constraints captured in the project context. Propose categories based on your understanding of the domain.",
            }));
          }
          return Promise.resolve(JSON.stringify({ synthesis }));
        }

        if (action === "persist_category") {
          const category = input["category"] as CategoryProposal;
          const decisions = input["decisions"] as ScopingDecision[];

          requirementsOrchestrator.persistCategory(projectId, category, decisions);

          const presentedCategories = (input["presentedCategories"] as string[] | undefined) ?? [];
          const allPresented = presentedCategories.includes(category.category)
            ? presentedCategories
            : [...presentedCategories, category.category];

          const covered = requirementsOrchestrator.validateCoverage(projectId, allPresented);

          log.info(`Persisted category ${category.category} for project ${projectId}; coverage=${covered}`);

          return Promise.resolve(JSON.stringify({
            persisted: decisions.length,
            category: category.category,
            coverageGate: covered,
            message: `Saved ${decisions.length} decisions for ${category.categoryName}. ${covered ? "Coverage gate met." : "Coverage gate not yet met."}`,
          }));
        }

        if (action === "update_requirement") {
          const reqId = input["reqId"] as string;
          const updates = input["updates"] as { tier?: "v1" | "v2" | "out-of-scope"; rationale?: string | null };

          requirementsOrchestrator.updateRequirement(projectId, reqId, updates);

          log.info(`Updated requirement ${reqId} for project ${projectId}`);

          return Promise.resolve(JSON.stringify({
            updated: reqId,
            changes: updates,
            message: `Requirement ${reqId} updated successfully.`,
          }));
        }

        if (action === "check_coverage") {
          const presentedCategories = (input["presentedCategories"] as string[]) ?? [];
          const covered = requirementsOrchestrator.validateCoverage(projectId, presentedCategories);

          return Promise.resolve(JSON.stringify({
            covered,
            presentedCategories,
            message: covered
              ? "Coverage gate met. All presented categories have decisions and at least one v1 requirement exists."
              : "Coverage gate not met. Ensure every presented category has at least one decision and at least one requirement is v1.",
          }));
        }

        if (action === "complete") {
          const presentedCategories = (input["presentedCategories"] as string[]) ?? [];
          const covered = requirementsOrchestrator.validateCoverage(projectId, presentedCategories);

          if (!covered) {
            return Promise.resolve(JSON.stringify({
              error: "Coverage gate not met. Cannot advance to roadmap until all presented categories have decisions and at least one v1 requirement exists.",
              covered: false,
            }));
          }

          const message = orchestrator.completeRequirements(projectId, context.nousId, context.sessionId);
          log.info(`Requirements complete for project ${projectId}; advancing to roadmap`);

          return Promise.resolve(JSON.stringify({
            complete: true,
            message,
          }));
        }

        return Promise.resolve(JSON.stringify({ error: `Unknown action: ${action}` }));
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log.error(`plan_requirements failed: ${message}`);
        return Promise.resolve(JSON.stringify({ error: message }));
      }
    },
  };
}
