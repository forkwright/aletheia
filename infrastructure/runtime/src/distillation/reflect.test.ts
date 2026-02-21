import { beforeEach, describe, expect, it, vi } from "vitest";
import { computeSelfAssessment, type ReflectionOpts, reflectOnAgent, weeklyReflection } from "./reflect.js";
import { SessionStore } from "../mneme/store.js";
import type { ProviderRouter } from "../hermeneus/router.js";

// --- Helpers ---

function makeStore(): SessionStore {
  return new SessionStore(":memory:");
}

function makeRouter(responseJson: Record<string, unknown>): ProviderRouter {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: JSON.stringify(responseJson) }],
      usage: { inputTokens: 5000, outputTokens: 1000 },
    }),
    completeStreaming: vi.fn(),
    registerProvider: vi.fn(),
  } as unknown as ProviderRouter;
}

function defaultOpts(overrides?: Partial<ReflectionOpts>): ReflectionOpts {
  return {
    model: "claude-haiku-4-5-20251001",
    minHumanMessages: 2,
    lookbackHours: 24,
    ...overrides,
  };
}

function seedSession(store: SessionStore, nousId: string, msgCount: number): string {
  const session = store.createSession(nousId, "main");
  for (let i = 0; i < msgCount; i++) {
    store.appendMessage(
      session.id,
      i % 2 === 0 ? "user" : "assistant",
      `Message ${i}: ${i % 2 === 0 ? "User asks about topic " + i : "Agent responds about topic " + i}`,
      { tokenEstimate: 50 },
    );
  }
  return session.id;
}

// --- Tests ---

describe("reflectOnAgent", () => {
  let store: SessionStore;

  beforeEach(() => {
    store = makeStore();
  });

  it("skips reflection when no qualifying sessions exist", async () => {
    const router = makeRouter({});
    const result = await reflectOnAgent(store, router, "syn", defaultOpts());

    expect(result.sessionsReviewed).toBe(0);
    expect(result.messagesReviewed).toBe(0);
    expect(router.complete).not.toHaveBeenCalled();
  });

  it("skips reflection when recent reflection already exists", async () => {
    const router = makeRouter({});
    seedSession(store, "syn", 20);

    // Record a recent reflection
    store.recordReflection({
      nousId: "syn",
      sessionsReviewed: 1,
      messagesReviewed: 20,
      findings: { patterns: [], contradictions: [], corrections: [], preferences: [], relationships: [], unresolvedThreads: [] },
      memoriesStored: 0,
      tokensUsed: 1000,
      durationMs: 500,
      model: "haiku",
    });

    const result = await reflectOnAgent(store, router, "syn", defaultOpts());
    expect(result.sessionsReviewed).toBe(0);
    expect(router.complete).not.toHaveBeenCalled();
  });

  it("reflects on sessions with meaningful activity", async () => {
    seedSession(store, "syn", 20);

    const findings = {
      patterns: ["[HIGH] User prefers tables for data display"],
      contradictions: [],
      corrections: ["[MEDIUM] Torque spec corrected from 225 to 185 ft-lbs"],
      implicit_preferences: ["[HIGH] User values directness over politeness"],
      relationships: ["[HIGH] (Cody, works_at, Summus)"],
      unresolved_threads: ["[LOW] Question about Neo4j indexing never answered"],
    };

    const router = makeRouter(findings);
    const result = await reflectOnAgent(store, router, "syn", defaultOpts());

    expect(result.sessionsReviewed).toBe(1);
    expect(result.messagesReviewed).toBeGreaterThan(0);
    expect(result.findings.patterns).toHaveLength(1);
    expect(result.findings.corrections).toHaveLength(1);
    expect(result.findings.preferences).toHaveLength(1);
    expect(result.findings.relationships).toHaveLength(1);
    expect(result.findings.unresolvedThreads).toHaveLength(1);
    expect(router.complete).toHaveBeenCalledTimes(1);
  });

  it("stores HIGH and MEDIUM confidence findings in memory", async () => {
    seedSession(store, "syn", 20);

    const findings = {
      patterns: ["[HIGH] Pattern A", "[LOW] Pattern B"],
      contradictions: ["[MEDIUM] Contradiction C"],
      corrections: [],
      implicit_preferences: ["[MEDIUM] Pref D", "[HIGH] Pref E"],
      relationships: [],
      unresolved_threads: [],
    };

    const memoryTarget = {
      addMemories: vi.fn().mockResolvedValue({ added: 3, errors: 0 }),
    };

    const router = makeRouter(findings);
    const result = await reflectOnAgent(store, router, "syn", {
      ...defaultOpts(),
      memoryTarget,
    });

    expect(result.memoriesStored).toBe(3);
    // HIGH pattern + MEDIUM contradiction + HIGH preference (MEDIUM pref skipped — preferences need HIGH)
    expect(memoryTarget.addMemories).toHaveBeenCalledWith("syn", [
      "[reflection:pattern] Pattern A",
      "[reflection:contradiction] Contradiction C",
      "[reflection:preference] Pref E",
    ]);
  });

  it("records reflection in the log", async () => {
    seedSession(store, "syn", 20);

    const findings = {
      patterns: ["[HIGH] Test pattern"],
      contradictions: [],
      corrections: [],
      implicit_preferences: [],
      relationships: [],
      unresolved_threads: [],
    };

    const router = makeRouter(findings);
    await reflectOnAgent(store, router, "syn", defaultOpts());

    const log = store.getReflectionLog("syn");
    expect(log).toHaveLength(1);
    expect(log[0]!.nousId).toBe("syn");
    expect(log[0]!.patternsFound).toBe(1);
    expect(log[0]!.sessionsReviewed).toBe(1);
    expect(log[0]!.tokensUsed).toBeGreaterThan(0);
  });

  it("handles unparseable JSON gracefully", async () => {
    seedSession(store, "syn", 20);

    const router = {
      complete: vi.fn().mockResolvedValue({
        content: [{ type: "text", text: "This is not JSON at all" }],
        usage: { inputTokens: 1000, outputTokens: 500 },
      }),
      completeStreaming: vi.fn(),
      registerProvider: vi.fn(),
    } as unknown as ProviderRouter;

    const result = await reflectOnAgent(store, router, "syn", defaultOpts());

    expect(result.findings.patterns).toHaveLength(0);
    expect(result.findings.contradictions).toHaveLength(0);
    // Should still record in log (with empty findings)
    const log = store.getReflectionLog("syn");
    expect(log).toHaveLength(1);
  });

  it("provides existing memories to the reflection prompt", async () => {
    seedSession(store, "syn", 20);

    const router = makeRouter({
      patterns: [],
      contradictions: ["[HIGH] Memory says X but conversation shows Y"],
      corrections: [],
      implicit_preferences: [],
      relationships: [],
      unresolved_threads: [],
    });

    await reflectOnAgent(store, router, "syn", {
      ...defaultOpts(),
      existingMemories: ["Cody prefers tables", "Aletheia uses SQLite"],
    });

    const call = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0];
    expect(call.system).toContain("Cody prefers tables");
    expect(call.system).toContain("Aletheia uses SQLite");
  });

  it("ignores sessions below minHumanMessages threshold", async () => {
    // Only 2 messages (1 user, 1 assistant) — below default threshold of 2
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "user", "Quick question", { tokenEstimate: 10 });
    store.appendMessage(session.id, "assistant", "Quick answer", { tokenEstimate: 10 });

    const router = makeRouter({});
    // minHumanMessages = 3 means at least 3 user messages
    const result = await reflectOnAgent(store, router, "syn", {
      ...defaultOpts(),
      minHumanMessages: 3,
    });

    expect(result.sessionsReviewed).toBe(0);
    expect(router.complete).not.toHaveBeenCalled();
  });
});

describe("reflection store methods", () => {
  it("getActiveSessionsSince filters correctly", () => {
    const store = makeStore();
    const s1 = store.createSession("syn", "main");
    // 5 user messages
    for (let i = 0; i < 10; i++) {
      store.appendMessage(s1.id, i % 2 === 0 ? "user" : "assistant", `msg ${i}`, { tokenEstimate: 10 });
    }

    // Ephemeral session — should be excluded
    const s2 = store.createSession("syn", "spawn:test");
    for (let i = 0; i < 10; i++) {
      store.appendMessage(s2.id, "user", `spawn msg ${i}`, { tokenEstimate: 10 });
    }

    const since = new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString();
    const sessions = store.getActiveSessionsSince("syn", since, 3);

    // Only the primary session qualifies (5 user messages >= 3)
    expect(sessions).toHaveLength(1);
    expect(sessions[0]!.sessionKey).toBe("main");
  });

  it("getLastReflection returns most recent", () => {
    const store = makeStore();

    expect(store.getLastReflection("syn")).toBeNull();

    store.recordReflection({
      nousId: "syn",
      sessionsReviewed: 1,
      messagesReviewed: 50,
      findings: { patterns: ["p1"], contradictions: [], corrections: [], preferences: [], relationships: [], unresolvedThreads: [] },
      memoriesStored: 1,
      tokensUsed: 5000,
      durationMs: 1000,
      model: "haiku",
    });

    const last = store.getLastReflection("syn");
    expect(last).not.toBeNull();
    expect(last!.patternsFound).toBe(1);
    expect(last!.sessionsReviewed).toBe(1);
  });
});


describe("weeklyReflection", () => {
  it("returns empty when no distillation summaries exist", async () => {
    const store = makeStore();
    const router = makeRouter({});

    const result = await weeklyReflection(store, router, "syn", {
      model: "claude-haiku-4-5-20251001",
    });

    expect(result.summariesReviewed).toBe(0);
    expect(result.trajectory).toHaveLength(0);
    expect(router.complete).not.toHaveBeenCalled();
  });

  it("reflects on distillation summaries from past week", async () => {
    const store = makeStore();
    const session = store.createSession("syn", "main");

    // Add some distillation summary messages
    store.appendMessage(session.id, "assistant", "[Distillation #1]\n\nUser worked on spec system and merged several PRs.", { tokenEstimate: 100 });
    store.appendMessage(session.id, "assistant", "[Distillation #2]\n\nUser shifted focus to gap analysis and reflection systems.", { tokenEstimate: 100 });

    const weeklyFindings = {
      trajectory: ["Focus shifted from spec system to reflection infrastructure"],
      topic_drift: ["Docker optimization mentioned early but dropped"],
      weekly_patterns: ["Deep technical work in evening hours"],
      unresolved_arcs: ["Mem0 sidecar connection issue persists"],
    };

    const router = makeRouter(weeklyFindings);
    const result = await weeklyReflection(store, router, "syn", {
      model: "claude-haiku-4-5-20251001",
    });

    expect(result.summariesReviewed).toBe(2);
    expect(result.trajectory).toHaveLength(1);
    expect(result.topicDrift).toHaveLength(1);
    expect(result.weeklyPatterns).toHaveLength(1);
    expect(result.unresolvedArcs).toHaveLength(1);
    expect(result.tokensUsed).toBeGreaterThan(0);
  });

  it("handles unparseable response gracefully", async () => {
    const store = makeStore();
    const session = store.createSession("syn", "main");
    store.appendMessage(session.id, "assistant", "[Distillation #1]\n\nSome summary.", { tokenEstimate: 50 });

    const router = {
      complete: vi.fn().mockResolvedValue({
        content: [{ type: "text", text: "Not valid JSON" }],
        usage: { inputTokens: 500, outputTokens: 200 },
      }),
      completeStreaming: vi.fn(),
      registerProvider: vi.fn(),
    } as unknown as ProviderRouter;

    const result = await weeklyReflection(store, router, "syn", {
      model: "claude-haiku-4-5-20251001",
    });

    expect(result.summariesReviewed).toBe(1);
    expect(result.trajectory).toHaveLength(0);
  });
});

describe("getDistillationSummaries", () => {
  it("returns only messages with Distillation # marker", () => {
    const store = makeStore();
    const session = store.createSession("syn", "main");

    store.appendMessage(session.id, "assistant", "Regular response", { tokenEstimate: 10 });
    store.appendMessage(session.id, "assistant", "[Distillation #1]\n\nSummary text", { tokenEstimate: 50 });
    store.appendMessage(session.id, "user", "Question about distillation", { tokenEstimate: 10 });
    store.appendMessage(session.id, "assistant", "[Distillation #2]\n\nAnother summary", { tokenEstimate: 50 });

    const since = new Date(Date.now() - 24 * 60 * 60 * 1000).toISOString();
    const summaries = store.getDistillationSummaries("syn", since);

    expect(summaries).toHaveLength(2);
    expect(summaries[0]!.summary).toContain("Distillation #2");
    expect(summaries[1]!.summary).toContain("Distillation #1");
  });
});


describe("computeSelfAssessment", () => {
  it("returns insufficient_data with fewer than 3 reflections", () => {
    const store = makeStore();
    const result = computeSelfAssessment(store, "syn");
    expect(result.trend).toBe("insufficient_data");
    expect(result.dataPoints).toBe(0);
  });

  it("computes correction rate and unresolved rate", () => {
    const store = makeStore();

    // Seed 4 reflections with varying findings
    for (let i = 0; i < 4; i++) {
      store.recordReflection({
        nousId: "syn",
        sessionsReviewed: 2,
        messagesReviewed: 100,
        findings: {
          patterns: ["p1"],
          contradictions: i < 2 ? ["c1"] : [],
          corrections: i % 2 === 0 ? ["fix1", "fix2"] : [],
          preferences: [],
          relationships: [],
          unresolvedThreads: i > 1 ? ["u1"] : [],
        },
        memoriesStored: 1,
        tokensUsed: 5000,
        durationMs: 1000,
        model: "haiku",
      });
    }

    const result = computeSelfAssessment(store, "syn");
    expect(result.dataPoints).toBe(4);
    expect(result.correctionRate).toBeGreaterThan(0); // 4 corrections / 8 sessions
    expect(result.contradictionCount).toBe(2);
    expect(result.trend).not.toBe("insufficient_data");
  });

  it("detects improving trend when recent reflections have fewer issues", () => {
    const store = makeStore();

    // Older reflections (lots of corrections)
    for (let i = 0; i < 4; i++) {
      store.recordReflection({
        nousId: "syn",
        sessionsReviewed: 1,
        messagesReviewed: 50,
        findings: {
          patterns: [],
          contradictions: [],
          corrections: ["fix1", "fix2", "fix3"],
          preferences: [],
          relationships: [],
          unresolvedThreads: ["u1", "u2"],
        },
        memoriesStored: 0,
        tokensUsed: 3000,
        durationMs: 500,
        model: "haiku",
      });
    }

    // Recent reflections (fewer corrections)
    for (let i = 0; i < 4; i++) {
      store.recordReflection({
        nousId: "syn",
        sessionsReviewed: 1,
        messagesReviewed: 50,
        findings: {
          patterns: [],
          contradictions: [],
          corrections: [],
          preferences: [],
          relationships: [],
          unresolvedThreads: [],
        },
        memoriesStored: 0,
        tokensUsed: 3000,
        durationMs: 500,
        model: "haiku",
      });
    }

    const result = computeSelfAssessment(store, "syn");
    // Recent (first 4 in DESC order = the zeros) vs older (last 4 = the 5-each)
    expect(result.trend).toBe("improving");
  });
});
