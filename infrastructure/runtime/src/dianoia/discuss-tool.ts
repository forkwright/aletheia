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
  dispatchTool?: ToolHandler,
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
            return handleGenerate(orchestrator, store, projectId, phaseId, context, dispatchTool);
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

/** Generate discussion questions for a phase using LLM analysis of requirements and context */
async function handleGenerate(
  orchestrator: DianoiaOrchestrator,
  store: PlanningStore,
  projectId: string,
  phaseId: string,
  context: ToolContext,
  dispatchTool?: ToolHandler,
): Promise<string> {
  const project = store.getProjectOrThrow(projectId);
  const phase = store.getPhaseOrThrow(phaseId);
  const allReqs = store.listRequirements(projectId);
  const phaseReqs = allReqs.filter((r) => phase.requirements.includes(r.reqId));

  // Build LLM prompt for question generation
  const prompt = [
    "You are a technical architect reviewing a project phase before implementation begins.",
    "Your job: identify 3-6 gray-area design decisions that could go multiple ways.",
    "Focus on decisions that would be EXPENSIVE to change later if chosen wrong.",
    "",
    "DO NOT ask about testing strategy, error handling approach, or sequential vs parallel — those are generic.",
    "DO ask about domain-specific ambiguities, tradeoffs between approaches, integration boundaries, and scope interpretation.",
    "",
    `## Project Goal`,
    project.goal,
    "",
    `## Phase: ${phase.name}`,
    `Goal: ${phase.goal}`,
    "",
    `## Success Criteria`,
    ...phase.successCriteria.map((c, i) => `${i + 1}. ${c}`),
    "",
    `## Requirements`,
    ...phaseReqs.map(r => `- ${r.reqId}: ${r.description} (${r.tier})`),
    "",
    "Respond with ONLY a JSON array of questions. Each question must have:",
    '  { "question": "...", "options": [{ "label": "...", "rationale": "..." }, ...], "recommendation": "label of recommended option" }',
    "",
    "Return 3-6 questions. No preamble, no explanation — just the JSON array.",
  ].join("\n");

  // Try LLM-powered generation first
  try {
    if (dispatchTool) {
      const raw = await dispatchTool.execute({
        tasks: [{
          role: "reviewer",
          task: prompt,
          timeoutSeconds: 120,
        }],
      }, context);

      const parsed = tryParseQuestions(raw as string);
      if (parsed && parsed.length > 0) {
        const created = [];
        for (const q of parsed) {
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

        log.info(`LLM generated ${created.length} discussion questions for phase ${phaseId}`);
        return JSON.stringify({
          generated: created.length,
          source: "llm",
          questions: created,
          message: `${created.length} questions generated. Answer with action=answer or skip with action=skip, then action=complete to advance.`,
        });
      }
    }
  } catch (err) {
    log.warn(`LLM question generation failed, falling back to heuristic`, {
      err: err instanceof Error ? err.message : String(err),
      phaseId,
    });
  }

  // Fallback: deterministic heuristic questions (better than nothing)
  const questions = generateHeuristicQuestions(phase, phaseReqs);
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

  log.info(`Heuristic generated ${created.length} discussion questions for phase ${phaseId}`);
  return JSON.stringify({
    generated: created.length,
    source: "heuristic",
    questions: created,
    message: `${created.length} questions generated (heuristic fallback). Answer with action=answer or skip with action=skip, then action=complete to advance.`,
  });
}

/** Try to parse LLM response as an array of discussion questions */
function tryParseQuestions(raw: string): Array<{
  question: string;
  options: Array<{ label: string; rationale: string }>;
  recommendation: string | null;
}> | null {
  try {
    // Try to extract JSON from dispatch response
    let text = raw;

    // If dispatch wraps in results envelope, unwrap
    try {
      const envelope = JSON.parse(raw);
      if (envelope.results?.[0]?.result) {
        text = envelope.results[0].result;
      }
    } catch { /* not an envelope */ }

    // Extract JSON array from text
    const trimmed = text.trim();

    // Direct array
    if (trimmed.startsWith("[")) {
      return JSON.parse(trimmed);
    }

    // Fenced json block
    const fenced = trimmed.match(/```(?:json)?\s*\n([\s\S]*?)\n```/);
    if (fenced?.[1]?.trim().startsWith("[")) {
      return JSON.parse(fenced[1].trim());
    }

    // Find array in text
    const arrayStart = trimmed.indexOf("[");
    const arrayEnd = trimmed.lastIndexOf("]");
    if (arrayStart >= 0 && arrayEnd > arrayStart) {
      return JSON.parse(trimmed.slice(arrayStart, arrayEnd + 1));
    }

    return null;
  } catch {
    return null;
  }
}

/** Deterministic fallback questions based on phase structure */
function generateHeuristicQuestions(
  phase: import("./types.js").PlanningPhase,
  phaseReqs: import("./types.js").PlanningRequirement[],
): Array<{
  question: string;
  options: Array<{ label: string; rationale: string }>;
  recommendation: string | null;
}> {
  const questions: Array<{
    question: string;
    options: Array<{ label: string; rationale: string }>;
    recommendation: string | null;
  }> = [];

  // Only generate questions about genuinely ambiguous success criteria
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

  // If phase has v2 requirements mixed in, ask about scope
  const v2Reqs = phaseReqs.filter(r => r.tier === "v2");
  if (v2Reqs.length > 0) {
    questions.push({
      question: `This phase includes ${v2Reqs.length} v2 requirement(s): ${v2Reqs.map(r => r.reqId).join(", ")}. Should we include them now or defer?`,
      options: [
        { label: "Include now", rationale: "Better to build while context is fresh" },
        { label: "Defer to v2", rationale: "Keep v1 scope tight, reduce risk" },
      ],
      recommendation: "Defer to v2",
    });
  }

  // Baseline: always generate an implementation approach question for non-trivial phases
  if (questions.length === 0 && (phaseReqs.length > 0 || phase.successCriteria.length > 0)) {
    questions.push({
      question: `What implementation approach for "${phase.name}"? The phase goal is: ${phase.goal}`,
      options: [
        { label: "Incremental", rationale: "Build feature by feature, validate as we go" },
        { label: "Foundation first", rationale: "Build core infrastructure, then layer features on top" },
      ],
      recommendation: "Incremental",
    });

    // If multiple requirements, ask about prioritization
    if (phaseReqs.length > 1) {
      questions.push({
        question: `This phase has ${phaseReqs.length} requirements. Which should be implemented first?`,
        options: phaseReqs.slice(0, 4).map(r => ({
          label: r.reqId,
          rationale: r.description,
        })),
        recommendation: phaseReqs[0]?.reqId ?? null,
      });
    }
  }

  return questions;
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
