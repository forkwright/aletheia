// Pass 2: Generate narrative summary for conversation continuity
import type { ProviderRouter } from "../hermeneus/router.js";
import type { ExtractionResult } from "./extract.js";

function buildSummaryPrompt(nousId?: string): string {
  const agentContext = nousId ? ` The reader is the ${nousId} agent,` : "";
  return `You are summarizing a conversation for continuity.${agentContext} the same AI assistant who will continue this conversation later.

Write a concise narrative summary that captures:
1. What was discussed and the intent behind it
2. Key decisions and their rationale
3. Specific facts from tool results (file contents, command outputs, search results)
4. Current state of any work in progress (be specific about what's done vs pending)
5. Open questions or blockers

Write in second person ("You were discussing...", "You decided to...").
Be specific — "You were editing bootstrap.ts to add hash tracking" is better than "You were working on code changes."
Preserve names, paths, numbers, and technical details exactly.

If this conversation has already been summarized before (you see a previous summary at the start), focus on what happened AFTER that summary — don't re-summarize the summary.

Keep it under 500 words. Focus on what's needed to continue the conversation effectively.`;
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
