// Pass 2: Generate narrative summary for conversation continuity
import type { ProviderRouter } from "../hermeneus/router.js";
import type { ExtractionResult } from "./extract.js";

const SUMMARY_PROMPT = `You are summarizing a conversation for continuity. The reader will be the same AI assistant continuing the conversation later.

Write a concise narrative summary that captures:
1. What was discussed and why
2. Key decisions made
3. Current state of any work in progress
4. Open questions or next steps

Write in second person ("You were discussing...", "You decided to...") to maintain continuity.
Keep it under 500 words. Focus on what's needed to continue the conversation effectively.`;

export async function summarizeMessages(
  router: ProviderRouter,
  messages: Array<{ role: string; content: string }>,
  extraction: ExtractionResult,
  model: string,
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
  ]
    .filter(Boolean)
    .join("\n");

  const userContent = extractionContext
    ? `Extracted context:\n${extractionContext}\n\nConversation:\n${conversation}`
    : conversation;

  const result = await router.complete({
    model,
    system: SUMMARY_PROMPT,
    messages: [{ role: "user", content: userContent }],
    maxTokens: 2048,
  });

  return result.content
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("");
}
