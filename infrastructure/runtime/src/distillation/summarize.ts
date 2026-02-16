// Pass 2: Generate narrative summary for conversation continuity
import type { ProviderRouter } from "../hermeneus/router.js";
import type { ExtractionResult } from "./extract.js";

function buildSummaryPrompt(nousId?: string): string {
  const agentContext = nousId ? ` You are ${nousId}.` : "";
  return `You are summarizing your own conversation for continuity.${agentContext} You will read this summary later to recall what you were doing.

Write a concise first-person narrative summary that captures:
1. What I was working on and why
2. Key decisions I made and my rationale
3. Specific facts from tool results (file contents, command outputs, search results)
4. Current state of work in progress (be specific about what's done vs pending)
5. Open questions or blockers I need to resolve

Write in first person ("I was discussing...", "I decided to...").
Be specific — "I was editing bootstrap.ts to add hash tracking" is better than "I was working on code changes."
Preserve names, paths, numbers, and technical details exactly.

If this conversation has already been summarized before (there is a previous summary at the start), focus on what happened AFTER that summary — don't re-summarize.

Keep it under 500 words. Focus on what I need to continue effectively.`;
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
