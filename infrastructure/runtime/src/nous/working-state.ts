// Working state extractor — lightweight post-turn extraction of task context
// Runs on a cheap model after each turn to maintain structured task state
// that survives distillation.
import { createLogger } from "../koina/logger.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { WorkingState } from "../mneme/store.js";

const log = createLogger("working-state");

function toStringArray(arr: unknown[]): string[] {
  return arr.filter((x): x is string => typeof x === "string");
}

const EXTRACTION_PROMPT = `You are extracting structured task context from a conversation turn. Output valid JSON only — no markdown, no explanation.

Given the assistant's last response and any tool calls, extract:

{
  "currentTask": "One-line description of what's being worked on right now",
  "completedSteps": ["Step that was just completed", "Another completed step"],
  "nextSteps": ["What needs to happen next"],
  "recentDecisions": ["Decision: X — because Y"],
  "openFiles": ["file/paths/mentioned.ts"]
}

Rules:
- currentTask: what is actively being worked on RIGHT NOW. Not history.
- completedSteps: only steps completed in THIS turn (max 5).
- nextSteps: immediate next actions, not long-term goals (max 3).
- recentDecisions: decisions made with brief rationale (max 3).
- openFiles: file paths referenced in tool calls (max 5).
- If a field has no content, use an empty array [].
- Keep everything concise — total output under 300 tokens.`;

export async function extractWorkingState(
  router: ProviderRouter,
  assistantText: string,
  toolSummary: string,
  previousState: WorkingState | null,
  model: string,
): Promise<WorkingState | null> {
  if (!assistantText && !toolSummary) return previousState;

  const input = [
    previousState
      ? `Previous working state:\n${JSON.stringify(previousState, null, 2)}\n\n`
      : "",
    toolSummary ? `Tool calls this turn:\n${toolSummary}\n\n` : "",
    `Assistant response:\n${assistantText.slice(0, 2000)}`,
  ]
    .filter(Boolean)
    .join("");

  try {
    const result = await router.complete({
      model,
      system: EXTRACTION_PROMPT,
      messages: [{ role: "user", content: input }],
      maxTokens: 512,
      temperature: 0,
    });

    const text = result.content
      .filter((b): b is { type: "text"; text: string } => b.type === "text")
      .map((b) => b.text)
      .join("");

    // Parse JSON — strip markdown fences if the model wraps it
    const cleaned = text
      .replace(/^```(?:json)?\s*\n?/m, "")
      .replace(/\n?```\s*$/m, "")
      .trim();

    const parsed = JSON.parse(cleaned) as Record<string, unknown>;

    const state: WorkingState = {
      currentTask: typeof parsed["currentTask"] === "string" ? parsed["currentTask"] : "",
      completedSteps: Array.isArray(parsed["completedSteps"])
        ? toStringArray(parsed["completedSteps"]).slice(0, 5)
        : [],
      nextSteps: Array.isArray(parsed["nextSteps"])
        ? toStringArray(parsed["nextSteps"]).slice(0, 3)
        : [],
      recentDecisions: Array.isArray(parsed["recentDecisions"])
        ? toStringArray(parsed["recentDecisions"]).slice(0, 3)
        : [],
      openFiles: Array.isArray(parsed["openFiles"])
        ? toStringArray(parsed["openFiles"]).slice(0, 5)
        : [],
      updatedAt: new Date().toISOString(),
    };

    // Don't store empty state
    if (!state.currentTask && state.completedSteps.length === 0 && state.nextSteps.length === 0) {
      return previousState;
    }

    return state;
  } catch (err) {
    log.debug(
      `Working state extraction failed (non-fatal): ${err instanceof Error ? err.message : err}`,
    );
    return previousState;
  }
}

export function formatWorkingState(state: WorkingState): string {
  const sections: string[] = [];

  if (state.currentTask) {
    sections.push(`**Current task:** ${state.currentTask}`);
  }

  if (state.completedSteps.length > 0) {
    sections.push(
      `**Completed:** ${state.completedSteps.map((s) => `${s}`).join("; ")}`,
    );
  }

  if (state.nextSteps.length > 0) {
    sections.push(
      `**Next:** ${state.nextSteps.map((s) => `${s}`).join("; ")}`,
    );
  }

  if (state.recentDecisions.length > 0) {
    sections.push(
      `**Decisions:** ${state.recentDecisions.map((d) => `${d}`).join("; ")}`,
    );
  }

  if (state.openFiles.length > 0) {
    sections.push(`**Files:** ${state.openFiles.join(", ")}`);
  }

  return `## Working State\n\n${sections.join("\n")}`;
}
