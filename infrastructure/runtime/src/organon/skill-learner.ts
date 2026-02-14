// Skill learning — extract reusable skills from successful multi-tool trajectories
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import type { ProviderRouter } from "../hermeneus/router.js";

const log = createLogger("organon.skill-learner");

const MIN_TOOL_CALLS = 3;
const RATE_LIMIT_MS = 60 * 60 * 1000; // 1 extraction per hour per agent

const lastExtraction = new Map<string, number>();

export interface ToolCallRecord {
  name: string;
  input: Record<string, unknown>;
  output: string;
}

export interface LearnedSkillCandidate {
  id: string;
  toolSequence: string[];
  name: string;
  description: string;
  instructions: string;
  sourceSession: string;
  sourceTurn: number;
}

function isRateLimited(nousId: string): boolean {
  const last = lastExtraction.get(nousId) ?? 0;
  return Date.now() - last < RATE_LIMIT_MS;
}

const EXTRACTION_PROMPT = `You are analyzing a successful tool call sequence from an AI agent.
If this sequence represents a REUSABLE pattern that could help in future similar tasks, generate a skill description.
If the sequence is too specific to be reusable (e.g., editing one particular file), return "NOT_GENERALIZABLE".

Tool sequence:
{SEQUENCE}

If generalizable, respond with EXACTLY this format (no other text):
---
# Skill Name
One-line description of what this skill does.

## When to Use
Describe when this skill pattern is applicable.

## Steps
1. Step one
2. Step two
...

## Tools Used
- tool_name: what it's used for in this pattern
---

If not generalizable, respond with just: NOT_GENERALIZABLE`;

export async function extractSkillCandidate(
  router: ProviderRouter,
  toolCalls: ToolCallRecord[],
  model: string,
  sessionId: string,
  turnSeq: number,
  nousId: string,
): Promise<LearnedSkillCandidate | null> {
  if (toolCalls.length < MIN_TOOL_CALLS) return null;
  if (toolCalls.some((tc) => tc.output.startsWith("Error"))) return null;
  if (isRateLimited(nousId)) return null;

  lastExtraction.set(nousId, Date.now());

  const sequence = toolCalls
    .map((tc, i) => `${i + 1}. ${tc.name}(${JSON.stringify(tc.input).slice(0, 200)}) → ${tc.output.slice(0, 200)}`)
    .join("\n");

  const prompt = EXTRACTION_PROMPT.replace("{SEQUENCE}", sequence);

  try {
    const result = await router.complete({
      model,
      system: "You extract reusable skill patterns from tool call sequences.",
      messages: [{ role: "user", content: prompt }],
      maxTokens: 1024,
    });

    const text = result.content
      .filter((b): b is { type: "text"; text: string } => b.type === "text")
      .map((b) => b.text)
      .join("");

    if (text.includes("NOT_GENERALIZABLE")) return null;

    // Parse the skill markdown
    const nameMatch = text.match(/^#\s+(.+)$/m);
    const descMatch = text.match(/^#\s+.+\n(.+)/m);

    if (!nameMatch) return null;

    const name = nameMatch[1]!.trim();
    const id = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
    const description = descMatch?.[1]?.trim() ?? name;

    // Extract content between --- markers, or use the whole response
    const fenceMatch = text.match(/---\n([\s\S]+?)\n---/);
    const instructions = fenceMatch ? fenceMatch[1]! : text;

    log.info(`Learned skill candidate: ${name} (${toolCalls.length} tools from ${nousId})`);

    return {
      id,
      toolSequence: toolCalls.map((tc) => tc.name),
      name,
      description,
      instructions,
      sourceSession: sessionId,
      sourceTurn: turnSeq,
    };
  } catch (err) {
    log.debug(`Skill extraction failed: ${err instanceof Error ? err.message : err}`);
    return null;
  }
}

export function saveLearnedSkill(candidate: LearnedSkillCandidate, skillsDir: string): void {
  const dir = join(skillsDir, candidate.id);
  if (existsSync(join(dir, "SKILL.md"))) {
    log.debug(`Skill ${candidate.id} already exists, skipping`);
    return;
  }

  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "SKILL.md"), candidate.instructions, "utf-8");
  log.info(`Saved learned skill: ${candidate.id} to ${dir}`);
}
