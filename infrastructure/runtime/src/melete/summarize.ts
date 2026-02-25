// Pass 2: Generate narrative summary for conversation continuity
import type { ProviderRouter } from "../hermeneus/router.js";
import type { ExtractionResult } from "./extract.js";

function buildSummaryPrompt(nousId?: string): string {
  const agentContext = nousId ? ` You are ${nousId}.` : "";
  return `You are summarizing your own conversation for continuity.${agentContext} You will read this summary later to recall what you were doing.

Write a structured summary using EXACTLY these sections. Include a section ONLY if there is content for it — omit empty sections entirely.

## Task Context
What the user asked for, what I was working on, and the goal. One to three sentences.

## Completed Work
What was accomplished, with specifics. Not "discussed PRs" but "reviewed and merged PRs #30-32, all clean, squash merged." Include file paths, command outputs, and technical details that matter.

## Key Decisions
Decisions made and WHY — the rationale matters as much as the decision. Format: "Decision: X. Reason: Y."

## Current State
Where we left off. What's in progress. What's half-done. Be specific about what's done vs. pending.

## Open Threads
Things mentioned but not yet addressed. Questions asked but not answered. Tasks deferred.

## Corrections
What was tried and didn't work. What was wrong and corrected. Prevents repeating mistakes.

Rules:
- Write in first person ("I was...", "I decided...").
- Be specific — "I was editing bootstrap.ts line 42 to add hash tracking" not "I was working on code."
- Preserve names, paths, numbers, and technical details exactly.
- If this conversation already has a previous summary, focus on what happened AFTER it.
- Keep each section concise. Total summary under 600 words.
- Do NOT include sections that have no content.`;
}

export async function summarizeMessages(
  router: ProviderRouter,
  messages: Array<{ role: string; content: string }>,
  extraction: ExtractionResult,
  model: string,
  nousId?: string,
): Promise<string> {
  const conversation = messages
    .map((m) => `${m.role}: ${m.content}`)
    .join("\n\n");

  const extractionContext = [
    extraction.facts.length > 0
      ? `Key facts: ${extraction.facts.join("; ")}`
      : "",
    extraction.decisions.length > 0
      ? `Decisions: ${extraction.decisions.join("; ")}`
      : "",
    extraction.openItems.length > 0
      ? `Open items: ${extraction.openItems.join("; ")}`
      : "",
    extraction.contradictions.length > 0
      ? `Contradictions: ${extraction.contradictions.join("; ")}`
      : "",
  ]
    .filter(Boolean)
    .join("\n");

  const userContent = extractionContext
    ? `Extracted context:\n${extractionContext}\n\nConversation:\n${conversation}`
    : conversation;

  const result = await router.complete({
    model,
    system: buildSummaryPrompt(nousId),
    messages: [{ role: "user", content: userContent }],
    maxTokens: 2048,
  });

  return result.content
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("");
}
