// Sleep-time reflection — deep pattern extraction from recent conversations
// Unlike real-time extraction (fast Haiku during distillation), reflection
// runs offline with more tokens and a purpose-built prompt that looks for
// patterns, contradictions, corrections, and implicit preferences.
import { createLogger } from "../koina/logger.js";
import { estimateTokens } from "../hermeneus/token-counter.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { ReflectionFindings, SessionStore } from "../mneme/store.js";
import type { MemoryFlushTarget } from "./hooks.js";
import { extractJson } from "./extract.js";
import { sanitizeToolResults } from "./chunked-summarize.js";

const log = createLogger("distillation.reflect");

const REFLECTION_PROMPT = `You are performing a nightly reflection on conversations from the Aletheia agent system.
This is NOT real-time extraction. You have time and space to think deeply about patterns.

Your job: look across all the messages below and find what fast extraction misses.

## What to look for

1. **PATTERNS** — Recurring themes, evolving opinions, consistent preferences across messages.
   - "User consistently chooses X over Y when Z is the constraint"
   - "User's interest in topic X has deepened across conversations"

2. **CONTRADICTIONS** — Information that conflicts with itself or with known context.
   - "User said A on Monday but B on Wednesday — these are incompatible"
   - "Agent claimed X but later evidence showed Y"
   Note: Include BOTH sides. Don't resolve — flag.

3. **CORRECTIONS** — Moments where wrong information was given and later corrected.
   - "Agent stated torque spec as 225 ft-lbs, user corrected to 185 ft-lbs"
   - "User initially said deadline was March, later corrected to February"
   Format: WRONG → RIGHT with context.

4. **IMPLICIT_PREFERENCES** — Things the user clearly prefers but never explicitly stated.
   - "User always asks for code examples rather than prose explanations"
   - "User consistently prefers tables for comparisons"
   Must be supported by multiple instances, not a single occurrence.

5. **RELATIONSHIPS** — Entity connections that were revealed or strengthened.
   - "PersonA works with PersonB on ProjectX"
   - "ToolA depends on ServiceB"
   Format as triples: (subject, relationship, object)

6. **UNRESOLVED_THREADS** — Questions asked but never answered, tasks mentioned but not tracked.
   - "User asked about X but conversation moved on without resolution"
   - "Agent committed to doing Y but no evidence it happened"

## Quality rules
- Each finding must be a single clear sentence.
- Patterns need at least 2 supporting instances to be real patterns (not coincidence).
- Implicit preferences need at least 3 instances.
- Contradictions must cite both conflicting statements.
- NEVER include findings that are obvious from a single message (that's extraction's job).
- Skip tool invocations, greetings, and meta-commentary about the conversation.
- Confidence: tag each finding as [HIGH], [MEDIUM], or [LOW].

## Existing memories for context
{EXISTING_MEMORIES}

If you find a contradiction WITH an existing memory, flag it explicitly.

Return ONLY valid JSON:
{
  "patterns": ["[CONFIDENCE] description"],
  "contradictions": ["[CONFIDENCE] description"],
  "corrections": ["[CONFIDENCE] description"],
  "implicit_preferences": ["[CONFIDENCE] description"],
  "relationships": ["[CONFIDENCE] (subject, relationship, object)"],
  "unresolved_threads": ["[CONFIDENCE] description"]
}`;

export interface ReflectionOpts {
  /** Model for reflection — should be capable of nuanced reasoning */
  model: string;
  /** Minimum human messages in 24h for a session to qualify */
  minHumanMessages: number;
  /** How far back to look (hours) */
  lookbackHours: number;
  /** Memory target for storing findings */
  memoryTarget?: MemoryFlushTarget;
  /** Existing memories to provide as context (for contradiction detection) */
  existingMemories?: string[];
  /** Maximum tokens per reflection chunk */
  maxChunkTokens?: number;
}

export interface ReflectionResult {
  nousId: string;
  sessionsReviewed: number;
  messagesReviewed: number;
  findings: ReflectionFindings;
  memoriesStored: number;
  tokensUsed: number;
  durationMs: number;
}

/**
 * Run reflection for a single nous — find deep patterns in recent conversations.
 */
export async function reflectOnAgent(
  store: SessionStore,
  router: ProviderRouter,
  nousId: string,
  opts: ReflectionOpts,
): Promise<ReflectionResult> {
  const startTime = Date.now();

  // Check if already reflected today
  const lastReflection = store.getLastReflection(nousId);
  if (lastReflection) {
    const hoursSince = (Date.now() - new Date(lastReflection.reflectedAt).getTime()) / (1000 * 60 * 60);
    if (hoursSince < opts.lookbackHours) {
      log.info(`Skipping reflection for ${nousId}: last reflection was ${Math.round(hoursSince)}h ago (window: ${opts.lookbackHours}h)`);
      return {
        nousId,
        sessionsReviewed: 0,
        messagesReviewed: 0,
        findings: emptyFindings(),
        memoriesStored: 0,
        tokensUsed: 0,
        durationMs: Date.now() - startTime,
      };
    }
  }

  // Find sessions with meaningful human activity
  const since = new Date(Date.now() - opts.lookbackHours * 60 * 60 * 1000).toISOString();
  const sessions = store.getActiveSessionsSince(nousId, since, opts.minHumanMessages);

  if (sessions.length === 0) {
    log.info(`No qualifying sessions for ${nousId} in last ${opts.lookbackHours}h`);
    return {
      nousId,
      sessionsReviewed: 0,
      messagesReviewed: 0,
      findings: emptyFindings(),
      memoriesStored: 0,
      tokensUsed: 0,
      durationMs: Date.now() - startTime,
    };
  }

  // Collect messages from qualifying sessions
  const allMessages: Array<{ role: string; content: string }> = [];
  let totalMessages = 0;

  for (const session of sessions) {
    const messages = store.getHistory(session.id, { excludeDistilled: true });
    const relevant = messages
      .filter((m) => m.role === "user" || m.role === "assistant" || m.role === "tool_result")
      .filter((m) => new Date(m.createdAt) >= new Date(since))
      .map((m) => {
        if (m.role === "tool_result") {
          const label = m.toolName ? `[tool:${m.toolName}]` : "[tool_result]";
          return { role: "user" as const, content: `${label} ${m.content}` };
        }
        return { role: m.role, content: m.content };
      });

    allMessages.push(...relevant);
    totalMessages += relevant.length;
  }

  if (allMessages.length === 0) {
    log.info(`No undistilled messages found for ${nousId} reflection`);
    return {
      nousId,
      sessionsReviewed: sessions.length,
      messagesReviewed: 0,
      findings: emptyFindings(),
      memoriesStored: 0,
      tokensUsed: 0,
      durationMs: Date.now() - startTime,
    };
  }

  // Sanitize tool results (truncate verbose outputs)
  const sanitized = sanitizeToolResults(allMessages);

  // Build the reflection prompt with existing memories
  const existingMemoriesText = opts.existingMemories?.length
    ? opts.existingMemories.map((m) => `- ${m}`).join("\n")
    : "(No existing memories provided — first reflection or memory service unavailable)";
  const systemPrompt = REFLECTION_PROMPT.replace("{EXISTING_MEMORIES}", existingMemoriesText);

  // Chunk if necessary
  const maxChunk = opts.maxChunkTokens ?? 80000;
  const totalTokens = sanitized.reduce((sum, m) => sum + estimateTokens(m.content), 0);
  let tokensUsed = 0;

  log.info(
    `Reflecting on ${nousId}: ${sessions.length} sessions, ${totalMessages} messages, ${totalTokens} tokens`,
  );

  let findings: ReflectionFindings;

  if (totalTokens <= maxChunk) {
    // Single-pass reflection
    const result = await reflectChunk(router, sanitized, systemPrompt, opts.model);
    findings = result.findings;
    tokensUsed = result.tokensUsed;
  } else {
    // Multi-chunk reflection with merge
    const chunks = splitByTokens(sanitized, maxChunk);
    log.info(`Chunked reflection: ${chunks.length} chunks`);

    const chunkResults: ReflectionFindings[] = [];
    for (let i = 0; i < chunks.length; i++) {
      log.info(`Reflecting chunk ${i + 1}/${chunks.length}`);
      const result = await reflectChunk(router, chunks[i]!, systemPrompt, opts.model);
      chunkResults.push(result.findings);
      tokensUsed += result.tokensUsed;
    }
    findings = mergeFindings(chunkResults);
  }

  const totalFindings =
    findings.patterns.length +
    findings.contradictions.length +
    findings.corrections.length +
    findings.preferences.length +
    findings.relationships.length +
    findings.unresolvedThreads.length;

  log.info(
    `Reflection complete for ${nousId}: ${totalFindings} findings ` +
    `(${findings.patterns.length} patterns, ${findings.contradictions.length} contradictions, ` +
    `${findings.corrections.length} corrections, ${findings.preferences.length} preferences, ` +
    `${findings.relationships.length} relationships, ${findings.unresolvedThreads.length} unresolved)`,
  );

  // Flush high-confidence findings to memory
  let memoriesStored = 0;
  if (opts.memoryTarget && totalFindings > 0) {
    memoriesStored = await flushReflectionToMemory(opts.memoryTarget, nousId, findings);
  }

  const durationMs = Date.now() - startTime;

  // Record in the reflection log
  store.recordReflection({
    nousId,
    sessionsReviewed: sessions.length,
    messagesReviewed: totalMessages,
    findings,
    memoriesStored,
    tokensUsed,
    durationMs,
    model: opts.model,
  });

  return {
    nousId,
    sessionsReviewed: sessions.length,
    messagesReviewed: totalMessages,
    findings,
    memoriesStored,
    tokensUsed,
    durationMs,
  };
}

/**
 * Run reflection on a chunk of messages and parse the result.
 */
async function reflectChunk(
  router: ProviderRouter,
  messages: Array<{ role: string; content: string }>,
  systemPrompt: string,
  model: string,
): Promise<{ findings: ReflectionFindings; tokensUsed: number }> {
  const conversation = messages
    .map((m) => `${m.role}: ${m.content}`)
    .join("\n\n");

  const result = await router.complete({
    model,
    system: systemPrompt,
    messages: [{ role: "user", content: conversation }],
    maxTokens: 8192,
    temperature: 0.3, // Slight creativity for pattern recognition
  });

  const tokensUsed = (result.usage?.inputTokens ?? 0) + (result.usage?.outputTokens ?? 0);

  const text = result.content
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("");

  const parsed = extractJson(text);
  if (!parsed) {
    log.warn(`Reflection returned no parseable JSON for chunk. Raw: ${text.slice(0, 300)}`);
    return { findings: emptyFindings(), tokensUsed };
  }

  return {
    findings: {
      patterns: asStringArray(parsed["patterns"]),
      contradictions: asStringArray(parsed["contradictions"]),
      corrections: asStringArray(parsed["corrections"]),
      preferences: asStringArray(parsed["implicit_preferences"]),
      relationships: asStringArray(parsed["relationships"]),
      unresolvedThreads: asStringArray(parsed["unresolved_threads"]),
    },
    tokensUsed,
  };
}

/**
 * Flush reflection findings to long-term memory.
 * Only stores HIGH and MEDIUM confidence findings.
 */
async function flushReflectionToMemory(
  target: MemoryFlushTarget,
  agentId: string,
  findings: ReflectionFindings,
): Promise<number> {
  const memories: string[] = [];

  // Patterns → stored as-is with reflection source tag
  for (const p of findings.patterns) {
    if (isHighOrMedium(p)) {
      memories.push(`[reflection:pattern] ${stripConfidence(p)}`);
    }
  }

  // Contradictions → stored with both sides
  for (const c of findings.contradictions) {
    if (isHighOrMedium(c)) {
      memories.push(`[reflection:contradiction] ${stripConfidence(c)}`);
    }
  }

  // Corrections → stored as corrected fact
  for (const c of findings.corrections) {
    if (isHighOrMedium(c)) {
      memories.push(`[reflection:correction] ${stripConfidence(c)}`);
    }
  }

  // Preferences → stored with high bar (only HIGH confidence)
  for (const p of findings.preferences) {
    if (isHigh(p)) {
      memories.push(`[reflection:preference] ${stripConfidence(p)}`);
    }
  }

  // Relationships → stored as triples
  for (const r of findings.relationships) {
    if (isHighOrMedium(r)) {
      memories.push(`[reflection:relationship] ${stripConfidence(r)}`);
    }
  }

  if (memories.length === 0) {
    log.info(`No high-confidence reflection findings to store for ${agentId}`);
    return 0;
  }

  try {
    const result = await target.addMemories(agentId, memories);
    log.info(`Reflection memory flush for ${agentId}: ${result.added} stored, ${result.errors} errors`);
    return result.added;
  } catch (err) {
    log.error(`Reflection memory flush failed for ${agentId}: ${err instanceof Error ? err.message : err}`);
    return 0;
  }
}

// --- Phase 2: Weekly cross-session reflection ---

const WEEKLY_REFLECTION_PROMPT = `You are performing a weekly reflection on an agent's distillation summaries from the past week.
These summaries capture what happened across multiple sessions. Your job is to find trajectory-level patterns.

## What to look for

1. **TRAJECTORY** — How did the user's focus shift across the week?
   - "Early in the week, focus was on X; shifted to Y after the decision about Z"
   - "Consistent daily attention to X, suggesting ongoing priority"

2. **TOPIC_DRIFT** — Things that were discussed but dropped.
   - "X was a topic Monday-Wednesday but hasn't appeared since"
   - "User asked about Y three times but never followed through"

3. **WEEKLY_PATTERNS** — Recurring weekly behaviors.
   - "User tends to do deep technical work early week and planning late week"
   - "Architecture conversations cluster on specific days"

4. **UNRESOLVED_ARC** — Multi-session threads that haven't concluded.
   - "Migration project mentioned in 3 sessions without completion marker"

Return ONLY valid JSON:
{
  "trajectory": ["description"],
  "topic_drift": ["description"],
  "weekly_patterns": ["description"],
  "unresolved_arcs": ["description"]
}`;

export interface WeeklyReflectionResult {
  nousId: string;
  summariesReviewed: number;
  trajectory: string[];
  topicDrift: string[];
  weeklyPatterns: string[];
  unresolvedArcs: string[];
  tokensUsed: number;
  durationMs: number;
}

/**
 * Weekly cross-session reflection over distillation summaries.
 * Looks for trajectory-level patterns across the past N days.
 */
export async function weeklyReflection(
  store: SessionStore,
  router: ProviderRouter,
  nousId: string,
  opts: {
    model: string;
    lookbackDays?: number;
  },
): Promise<WeeklyReflectionResult> {
  const startTime = Date.now();
  const lookbackDays = opts.lookbackDays ?? 7;
  const since = new Date(Date.now() - lookbackDays * 24 * 60 * 60 * 1000).toISOString();

  const summaries = store.getDistillationSummaries(nousId, since);

  if (summaries.length === 0) {
    log.info(`No distillation summaries for ${nousId} in last ${lookbackDays} days`);
    return {
      nousId,
      summariesReviewed: 0,
      trajectory: [],
      topicDrift: [],
      weeklyPatterns: [],
      unresolvedArcs: [],
      tokensUsed: 0,
      durationMs: Date.now() - startTime,
    };
  }

  // Build the conversation from summaries (chronological)
  const conversation = summaries
    .reverse() // oldest first
    .map((s) => {
      const date = new Date(s.createdAt).toLocaleDateString("en-US", { weekday: "short", month: "short", day: "numeric" });
      return `[${date}] ${s.summary}`;
    })
    .join("\n\n---\n\n");

  log.info(`Weekly reflection for ${nousId}: ${summaries.length} summaries, ${lookbackDays} day window`);

  const result = await router.complete({
    model: opts.model,
    system: WEEKLY_REFLECTION_PROMPT,
    messages: [{ role: "user", content: conversation }],
    maxTokens: 4096,
    temperature: 0.3,
  });

  const tokensUsed = (result.usage?.inputTokens ?? 0) + (result.usage?.outputTokens ?? 0);

  const text = result.content
    .filter((b): b is { type: "text"; text: string } => b.type === "text")
    .map((b) => b.text)
    .join("");

  const parsed = extractJson(text);
  if (!parsed) {
    log.warn(`Weekly reflection returned no parseable JSON. Raw: ${text.slice(0, 300)}`);
    return {
      nousId,
      summariesReviewed: summaries.length,
      trajectory: [],
      topicDrift: [],
      weeklyPatterns: [],
      unresolvedArcs: [],
      tokensUsed,
      durationMs: Date.now() - startTime,
    };
  }

  const weeklyResult: WeeklyReflectionResult = {
    nousId,
    summariesReviewed: summaries.length,
    trajectory: asStringArray(parsed["trajectory"]),
    topicDrift: asStringArray(parsed["topic_drift"]),
    weeklyPatterns: asStringArray(parsed["weekly_patterns"]),
    unresolvedArcs: asStringArray(parsed["unresolved_arcs"]),
    tokensUsed,
    durationMs: Date.now() - startTime,
  };

  const totalFindings = weeklyResult.trajectory.length +
    weeklyResult.topicDrift.length +
    weeklyResult.weeklyPatterns.length +
    weeklyResult.unresolvedArcs.length;

  log.info(
    `Weekly reflection for ${nousId}: ${totalFindings} findings ` +
    `(${weeklyResult.trajectory.length} trajectory, ${weeklyResult.topicDrift.length} drift, ` +
    `${weeklyResult.weeklyPatterns.length} patterns, ${weeklyResult.unresolvedArcs.length} arcs)`,
  );

  return weeklyResult;
}

// --- Phase 3: Self-Assessment Integration ---

export interface SelfAssessment {
  nousId: string;
  /** How often this agent gets corrected (lower = more calibrated) */
  correctionRate: number;
  /** How many unresolved threads accumulate (lower = better attention) */
  unresolvedRate: number;
  /** Number of contradictions detected (indicates memory quality issues) */
  contradictionCount: number;
  /** Trend: improving, stable, or degrading based on recent reflections */
  trend: "improving" | "stable" | "degrading" | "insufficient_data";
  /** Raw data points */
  dataPoints: number;
}

/**
 * Compute a self-assessment from recent reflection logs.
 * Looks at the last N reflections to derive calibration signals.
 */
export function computeSelfAssessment(
  store: SessionStore,
  nousId: string,
  opts?: { limit?: number },
): SelfAssessment {
  const limit = opts?.limit ?? 14; // two weeks of nightly reflections
  const reflections = store.getReflectionLog(nousId, { limit });

  if (reflections.length < 3) {
    return {
      nousId,
      correctionRate: 0,
      unresolvedRate: 0,
      contradictionCount: 0,
      trend: "insufficient_data",
      dataPoints: reflections.length,
    };
  }

  // Compute rates across all reflections
  const totalSessions = reflections.reduce((sum, r) => sum + r.sessionsReviewed, 0);
  const totalCorrections = reflections.reduce((sum, r) => sum + r.correctionsFound, 0);
  const totalUnresolved = reflections.reduce((sum, r) => sum + r.unresolvedThreadsFound, 0);
  const totalContradictions = reflections.reduce((sum, r) => sum + r.contradictionsFound, 0);

  const correctionRate = totalSessions > 0 ? totalCorrections / totalSessions : 0;
  const unresolvedRate = totalSessions > 0 ? totalUnresolved / totalSessions : 0;

  // Compute trend: compare first half vs second half of reflections
  const mid = Math.floor(reflections.length / 2);
  // reflections are in DESC order (newest first)
  const recent = reflections.slice(0, mid);
  const older = reflections.slice(mid);

  const recentCorrections = recent.reduce((sum, r) => sum + r.correctionsFound, 0);
  const olderCorrections = older.reduce((sum, r) => sum + r.correctionsFound, 0);
  const recentUnresolved = recent.reduce((sum, r) => sum + r.unresolvedThreadsFound, 0);
  const olderUnresolved = older.reduce((sum, r) => sum + r.unresolvedThreadsFound, 0);

  // Lower is better for both metrics
  const recentScore = recentCorrections + recentUnresolved;
  const olderScore = olderCorrections + olderUnresolved;

  let trend: SelfAssessment["trend"];
  if (recentScore < olderScore * 0.7) {
    trend = "improving";
  } else if (recentScore > olderScore * 1.3) {
    trend = "degrading";
  } else {
    trend = "stable";
  }

  return {
    nousId,
    correctionRate: Math.round(correctionRate * 100) / 100,
    unresolvedRate: Math.round(unresolvedRate * 100) / 100,
    contradictionCount: totalContradictions,
    trend,
    dataPoints: reflections.length,
  };
}

function emptyFindings(): ReflectionFindings {
  return {
    patterns: [],
    contradictions: [],
    corrections: [],
    preferences: [],
    relationships: [],
    unresolvedThreads: [],
  };
}

function mergeFindings(results: ReflectionFindings[]): ReflectionFindings {
  const merged = emptyFindings();
  for (const r of results) {
    merged.patterns.push(...r.patterns);
    merged.contradictions.push(...r.contradictions);
    merged.corrections.push(...r.corrections);
    merged.preferences.push(...r.preferences);
    merged.relationships.push(...r.relationships);
    merged.unresolvedThreads.push(...r.unresolvedThreads);
  }
  // Deduplicate by exact match
  merged.patterns = [...new Set(merged.patterns)];
  merged.contradictions = [...new Set(merged.contradictions)];
  merged.corrections = [...new Set(merged.corrections)];
  merged.preferences = [...new Set(merged.preferences)];
  merged.relationships = [...new Set(merged.relationships)];
  merged.unresolvedThreads = [...new Set(merged.unresolvedThreads)];
  return merged;
}

function splitByTokens(
  messages: Array<{ role: string; content: string }>,
  maxTokensPerChunk: number,
): Array<Array<{ role: string; content: string }>> {
  const chunks: Array<Array<{ role: string; content: string }>> = [];
  let current: Array<{ role: string; content: string }> = [];
  let currentTokens = 0;

  for (const msg of messages) {
    const tokens = estimateTokens(msg.content);
    if (currentTokens + tokens > maxTokensPerChunk && current.length > 0) {
      chunks.push(current);
      current = [];
      currentTokens = 0;
    }
    current.push(msg);
    currentTokens += tokens;
  }
  if (current.length > 0) chunks.push(current);
  return chunks;
}

function asStringArray(val: unknown): string[] {
  return Array.isArray(val) ? val.filter((v): v is string => typeof v === "string") : [];
}

function isHigh(finding: string): boolean {
  return finding.startsWith("[HIGH]");
}

function isHighOrMedium(finding: string): boolean {
  return finding.startsWith("[HIGH]") || finding.startsWith("[MEDIUM]");
}

function stripConfidence(finding: string): string {
  return finding.replace(/^\[(HIGH|MEDIUM|LOW)\]\s*/, "");
}
