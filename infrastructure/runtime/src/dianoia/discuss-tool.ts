// plan_discuss tool — manage per-phase discussion: generate, present, answer, skip, complete
// This bridges the 'discussing' FSM state between roadmap and phase-planning.
import { createLogger } from "../koina/logger.js";
import type { ToolContext, ToolHandler } from "../organon/registry.js";
import type { DianoiaOrchestrator } from "./orchestrator.js";
import { PlanningStore } from "./store.js";
import type Database from "better-sqlite3";

const log = createLogger("dianoia:discuss-tool");

export function createPlanDiscussTool(
  orchestrator: DianoiaOrchestrator,
  db: Database.Database,
): ToolHandler {
  const store = new PlanningStore(db);

  return {
    definition: {
      name: "plan_discuss",
      description:
        "Manage the per-phase discussion flow. Generate gray-area questions, present them for decisions, " +
        "collect answers, and complete the discussion to advance to phase planning.\n\n" +
        "Actions:\n" +
        "- generate: Auto-generate discussion questions for a phase based on requirements and context\n" +
        "- list: Show all discussion questions for a phase\n" +
        "- add: Manually add a discussion question\n" +
        "- answer: Answer a specific question with a decision\n" +
        "- skip: Skip a question (agent uses its recommendation)\n" +
        "- complete: Finalize discussion and advance to phase planning",
      input_schema: {
        type: "object",
        properties: {
          action: {
            type: "string",
            enum: ["generate", "list", "add", "answer", "skip", "complete"],
            description: "Action to perform",
          },
          projectId: {
            type: "string",
            description: "Active planning project ID",
          },
          phaseId: {
            type: "string",
            description: "Phase ID for the discussion",
          },
          questionId: {
            type: "string",
            description: "Discussion question ID (for answer/skip actions)",
          },
          question: {
            type: "string",
            description: "Question text (for add action)",
          },
          options: {
            type: "array",
            description: "Options for the question (for add action)",
            items: {
              type: "object",
              properties: {
                label: { type: "string" },
                rationale: { type: "string" },
              },
              required: ["label", "rationale"],
            },
          },
          recommendation: {
            type: "string",
            description: "Recommended option (for add action)",
          },
          decision: {
            type: "string",
            description: "Decision text (for answer action)",
          },
          userNote: {
            type: "string",
            description: "Optional note explaining the decision (for answer action)",
          },
        },
        required: ["action", "projectId"],
      },
    },
    async execute(input: Record<string, unknown>, context: ToolContext): Promise<string> {
      const action = input["action"] as string;
      const projectId = input["projectId"] as string;
      const phaseId = input["phaseId"] as string | undefined;

      try {
        switch (action) {
          case "generate": {
            if (!phaseId) return JSON.stringify({ error: "phaseId required for generate" });
            return handleGenerate(orchestrator, store, projectId, phaseId, context);
          }

          case "list": {
            if (!phaseId) return JSON.stringify({ error: "phaseId required for list" });
            return handleList(orchestrator, projectId, phaseId);
          }

          case "add": {
            if (!phaseId) return JSON.stringify({ error: "phaseId required for add" });
            const question = input["question"] as string;
            if (!question) return JSON.stringify({ error: "question required for add" });
            const options = (input["options"] as Array<{ label: string; rationale: string }>) ?? [];
            const recommendation = (input["recommendation"] as string) ?? null;

            const q = orchestrator.addDiscussionQuestion(projectId, phaseId, question, options, recommendation);
            log.info(`Added discussion question ${q.id} for phase ${phaseId}`);
            return JSON.stringify({
              questionId: q.id,
              question: q.question,
              options: q.options,
              recommendation: q.recommendation,
              status: q.status,
            });
          }

          case "answer": {
            const questionId = input["questionId"] as string;
            if (!questionId) return JSON.stringify({ error: "questionId required for answer" });
            const decision = input["decision"] as string;
            if (!decision) return JSON.stringify({ error: "decision required for answer" });
            const userNote = (input["userNote"] as string) ?? null;

            orchestrator.answerDiscussion(questionId, decision, userNote);
            log.info(`Answered discussion question ${questionId}: ${decision}`);
            return JSON.stringify({ answered: true, questionId, decision });
          }

          case "skip": {
            const questionId = input["questionId"] as string;
            if (!questionId) return JSON.stringify({ error: "questionId required for skip" });

            orchestrator.skipDiscussion(questionId);
            log.info(`Skipped discussion question ${questionId}`);
            return JSON.stringify({ skipped: true, questionId });
          }

          case "complete": {
            if (!phaseId) return JSON.stringify({ error: "phaseId required for complete" });

            // Check all questions are resolved
            const pending = orchestrator.getPendingDiscussions(projectId, phaseId);
            if (pending.length > 0) {
              return JSON.stringify({
                error: "Unresolved questions remain",
                pendingCount: pending.length,
                pendingQuestions: pending.map((q) => ({ id: q.id, question: q.question })),
                message: "Answer or skip all pending questions before completing discussion.",
              });
            }

            const result = orchestrator.completeDiscussion(projectId, phaseId, context.nousId, context.sessionId);
            log.info(`Discussion completed for phase ${phaseId}`);
            return JSON.stringify({
              complete: true,
              message: result,
              nextState: "phase-planning",
            });
          }

          default:
            return JSON.stringify({ error: `Unknown action: ${action}` });
        }
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log.error(`plan_discuss [${action}] failed: ${message}`);
        return JSON.stringify({ error: message });
      }
    },
  };
}

/** Generate discussion questions for a phase by analyzing requirements and context */
async function handleGenerate(
  orchestrator: DianoiaOrchestrator,
  store: PlanningStore,
  projectId: string,
  phaseId: string,
  _context: ToolContext,
): Promise<string> {
  store.getProjectOrThrow(projectId); // Validate project exists
  const phase = store.getPhaseOrThrow(phaseId);
  const allReqs = store.listRequirements(projectId);
  const phaseReqs = allReqs.filter((r) => phase.requirements.includes(r.reqId));

  // Build questions from phase analysis (deterministic, no LLM needed)
  const questions: Array<{
    question: string;
    options: Array<{ label: string; rationale: string }>;
    recommendation: string | null;
  }> = [];

  // 1. Implementation approach question (if phase has multiple requirements)
  if (phaseReqs.length >= 2) {
    questions.push({
      question: `For "${phase.name}": should requirements be implemented sequentially or in parallel?`,
      options: [
        { label: "Sequential", rationale: "Simpler dependency management, easier to review" },
        { label: "Parallel", rationale: "Faster overall delivery, but more complex integration" },
      ],
      recommendation: phaseReqs.length > 4 ? "Sequential" : "Parallel",
    });
  }

  // 2. Testing strategy question
  questions.push({
    question: `Testing strategy for "${phase.name}": what level of test coverage?`,
    options: [
      { label: "Unit tests only", rationale: "Fast, focused, catches regressions" },
      { label: "Unit + integration", rationale: "Covers interactions between components" },
      { label: "Full: unit + integration + e2e", rationale: "Comprehensive but slower to write" },
    ],
    recommendation: "Unit + integration",
  });

  // 3. Error handling approach
  questions.push({
    question: `Error handling for "${phase.name}": fail-fast or graceful degradation?`,
    options: [
      { label: "Fail-fast", rationale: "Clear error signals, simpler logic, easier debugging" },
      { label: "Graceful degradation", rationale: "Better UX, handles partial failures, more complex" },
    ],
    recommendation: "Fail-fast",
  });

  // 4. Phase-specific: if phase has success criteria with ambiguity
  for (const criterion of phase.successCriteria) {
    if (criterion.includes("or") || criterion.includes("configurable") || criterion.includes("optional")) {
      questions.push({
        question: `Success criterion "${criterion}" has flexibility — which interpretation should guide implementation?`,
        options: [
          { label: "Minimal viable", rationale: "Implement the simplest valid interpretation" },
          { label: "Comprehensive", rationale: "Cover all interpretations of this criterion" },
        ],
        recommendation: "Minimal viable",
      });
    }
  }

  // Persist all generated questions
  const created = [];
  for (const q of questions) {
    const disc = orchestrator.addDiscussionQuestion(
      projectId, phaseId, q.question, q.options, q.recommendation,
    );
    created.push({
      id: disc.id,
      question: disc.question,
      options: disc.options,
      recommendation: disc.recommendation,
    });
  }

  log.info(`Generated ${created.length} discussion questions for phase ${phaseId}`);
  return JSON.stringify({
    generated: created.length,
    questions: created,
    message: `${created.length} questions generated. Answer with action=answer or skip with action=skip, then action=complete to advance.`,
  });
}

/** List all discussion questions for a phase with status */
function handleList(
  orchestrator: DianoiaOrchestrator,
  projectId: string,
  phaseId: string,
): string {
  const all = orchestrator.getPhaseDiscussions(projectId, phaseId);
  const pending = orchestrator.getPendingDiscussions(projectId, phaseId);

  const formatted = all.map((q) => ({
    id: q.id,
    question: q.question,
    options: q.options,
    recommendation: q.recommendation,
    status: q.status,
    decision: q.decision,
    userNote: q.userNote,
  }));

  return JSON.stringify({
    total: all.length,
    pending: pending.length,
    answered: all.filter((q) => q.status === "answered").length,
    skipped: all.filter((q) => q.status === "skipped").length,
    questions: formatted,
  });
}
