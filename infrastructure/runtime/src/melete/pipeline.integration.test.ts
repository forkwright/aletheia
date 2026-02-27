// E2E integration tests for all memory write paths
import { afterEach, describe, expect, it, vi } from "vitest";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { randomUUID } from "node:crypto";
import type { ProviderRouter } from "../hermeneus/router.js";

// --- Helpers ---

function makeRouter(responseText: string): ProviderRouter {
  return {
    complete: vi.fn().mockResolvedValue({
      content: [{ type: "text", text: responseText }],
      usage: { inputTokens: 500, outputTokens: 200 },
    }),
    completeStreaming: vi.fn(),
    registerProvider: vi.fn(),
  } as unknown as ProviderRouter;
}

const tempDirs: string[] = [];

function tmpDir(): string {
  const d = mkdtempSync(join(tmpdir(), `pipeline-integration-${randomUUID().slice(0, 8)}-`));
  tempDirs.push(d);
  return d;
}

afterEach(() => {
  for (const d of tempDirs.splice(0)) {
    try { rmSync(d, { recursive: true }); } catch { /* ignore */ }
  }
  vi.clearAllMocks();
  vi.unstubAllGlobals();
});

// --- Write Path 1: Per-turn extraction (extractTurnFacts) ---

describe("write path: per-turn extraction (extractTurnFacts)", () => {
  it("extracts facts from a substantive assistant response", async () => {
    const { extractTurnFacts } = await import("../nous/turn-facts.js");

    const facts = [
      "Widget torque is 185 ft-lbs on the 2003 Honda Passport",
      "Chrome-tanned leather preferred over veg-tan for belt durability",
    ];
    const router = makeRouter(JSON.stringify(facts));

    const assistantText = "The Widget torque spec for your 2003 Honda Passport is 185 ft-lbs, not 225. " +
      "Also, for belt making, polymer leather holds up much better than veg-tan for daily wear. ".repeat(3);

    const result = await extractTurnFacts(router, assistantText, "", "claude-haiku-4-5-20251001");

    expect(result).toHaveProperty("facts");
    expect(result).toHaveProperty("model", "claude-haiku-4-5-20251001");
    expect(result).toHaveProperty("durationMs");
    expect(Array.isArray(result.facts)).toBe(true);
  });

  it("calls the router with prompt containing the response text", async () => {
    const { extractTurnFacts } = await import("../nous/turn-facts.js");

    const router = makeRouter("[]");
    const assistantText = "Project Alpha is due in October 2026, which means planning needs to account for reduced capacity from September onward. "
      .repeat(5);

    await extractTurnFacts(router, assistantText, "", "claude-haiku-4-5-20251001");

    expect(router.complete).toHaveBeenCalledTimes(1);
    const call = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0];
    expect(call.messages[0]!.content).toContain("Project Alpha is due in October 2026");
  });

  it("includes tool summary in the router call when provided", async () => {
    const { extractTurnFacts } = await import("../nous/turn-facts.js");

    const router = makeRouter("[]");
    const assistantText = "Based on the grep results, the auth module has 6 unused exports that should be cleaned up before the next release. "
      .repeat(4);
    const toolSummary = "grep(pattern='export') → found 6 unused exports in auth/index.ts";

    await extractTurnFacts(router, assistantText, toolSummary, "claude-haiku-4-5-20251001");

    const call = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0];
    expect(call.messages[0]!.content).toContain("Tool Results");
    expect(call.messages[0]!.content).toContain("grep(pattern=");
  });

  it("skips the router call for short responses", async () => {
    const { extractTurnFacts } = await import("../nous/turn-facts.js");

    const router = makeRouter("[]");
    const result = await extractTurnFacts(router, "Short reply.", "", "claude-haiku-4-5-20251001");

    expect(router.complete).not.toHaveBeenCalled();
    expect(result.facts).toHaveLength(0);
  });

  it("filters noise patterns from extracted facts", async () => {
    const { extractTurnFacts } = await import("../nous/turn-facts.js");

    const noisyFacts = [
      "Uses grep to search for patterns",
      "Runs the test suite to verify behavior",
      "Widget torque is 185 ft-lbs on the 2003 Honda Passport",
    ];
    const router = makeRouter(JSON.stringify(noisyFacts));

    const assistantText = "The auth module needs cleanup. " +
      "Widget torque spec is an important thing to know for the vehicle maintenance section. ".repeat(5);

    const result = await extractTurnFacts(router, assistantText, "", "claude-haiku-4-5-20251001");

    const noiseFact1 = result.facts.find((f) => f.startsWith("Uses grep"));
    const noiseFact2 = result.facts.find((f) => f.startsWith("Runs the"));
    expect(noiseFact1).toBeUndefined();
    expect(noiseFact2).toBeUndefined();
  });
});

// --- Write Path 2: Distillation extraction (extractFromMessages) ---

describe("write path: distillation extraction (extractFromMessages)", () => {
  it("returns full ExtractionResult with all fields from a conversation", async () => {
    const { extractFromMessages } = await import("./extract.js");

    const extractionJson = {
      facts: ["Prosoche dedup window set to 8 hours to reduce alert fatigue"],
      decisions: ["Switch from polling to event-driven updates for better performance"],
      openItems: ["Evaluate Mem0 sidecar usage during Phase 2"],
      keyEntities: ["Aletheia", "Qdrant", "Neo4j"],
      contradictions: [],
    };
    const router = makeRouter(JSON.stringify(extractionJson));

    const messages = [
      { role: "user", content: "Let's review the Prosoche dedup window — it's causing too many alerts." },
      { role: "assistant", content: "The 8-hour dedup window should reduce that alert fatigue significantly." },
      { role: "user", content: "Should we switch to event-driven updates?" },
      { role: "assistant", content: "Yes, event-driven updates would be more efficient than polling in this architecture." },
    ];

    const result = await extractFromMessages(router, messages, "claude-haiku-4-5-20251001");

    expect(result).toHaveProperty("facts");
    expect(result).toHaveProperty("decisions");
    expect(result).toHaveProperty("openItems");
    expect(result).toHaveProperty("keyEntities");
    expect(result).toHaveProperty("contradictions");
    expect(result.facts).toEqual(extractionJson.facts);
    expect(result.decisions).toEqual(extractionJson.decisions);
    expect(result.openItems).toEqual(extractionJson.openItems);
    expect(result.keyEntities).toEqual(extractionJson.keyEntities);
  });

  it("calls the router with the conversation content in messages", async () => {
    const { extractFromMessages } = await import("./extract.js");

    const router = makeRouter(JSON.stringify({
      facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [],
    }));

    const messages = [
      { role: "user", content: "How does the distillation pipeline work?" },
      { role: "assistant", content: "The distillation pipeline extracts facts and decisions from conversation history." },
    ];

    await extractFromMessages(router, messages, "claude-haiku-4-5-20251001");

    expect(router.complete).toHaveBeenCalledTimes(1);
    const call = (router.complete as ReturnType<typeof vi.fn>).mock.calls[0]![0];
    expect(JSON.stringify(call)).toContain("distillation pipeline");
  });

  it("handles unparseable JSON gracefully by returning empty result", async () => {
    const { extractFromMessages } = await import("./extract.js");

    const router = makeRouter("This is not JSON at all — just prose.");
    const messages = [
      { role: "user", content: "Some question?" },
      { role: "assistant", content: "Some answer." },
    ];

    const result = await extractFromMessages(router, messages, "claude-haiku-4-5-20251001");

    expect(result.facts).toEqual([]);
    expect(result.decisions).toEqual([]);
    expect(result.openItems).toEqual([]);
    expect(result.keyEntities).toEqual([]);
    expect(result.contradictions).toEqual([]);
  });

  it("filters noise from extracted facts", async () => {
    const { extractFromMessages } = await import("./extract.js");

    const extractionJson = {
      facts: [
        "Uses grep",
        "The user asked about configuration",
        "ALETHEIA_MEMORY_USER must be set in aletheia.env or all extractions default to user_id='default'",
      ],
      decisions: [],
      openItems: [],
      keyEntities: [],
      contradictions: [],
    };
    const router = makeRouter(JSON.stringify(extractionJson));

    const messages = [
      { role: "user", content: "What env var controls the memory user?" },
      { role: "assistant", content: "ALETHEIA_MEMORY_USER must be set in aletheia.env or all extractions default to user_id='default'." },
    ];

    const result = await extractFromMessages(router, messages, "claude-haiku-4-5-20251001");

    expect(result.facts.find((f) => f.includes("The user asked"))).toBeUndefined();
    expect(result.facts.find((f) => f.includes("ALETHEIA_MEMORY_USER"))).toBeDefined();
  });
});

// --- Write Path 3: Workspace flush (flushToWorkspace) ---

describe("write path: workspace flush (flushToWorkspace)", () => {
  it("writes markdown file and returns written=true with a valid path", async () => {
    const { flushToWorkspace } = await import("./workspace-flush.js");
    const workspace = tmpDir();

    const result = flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: randomUUID(),
      distillationNumber: 1,
      summary: "Integration test session — reviewed memory pipeline wiring.",
      extraction: {
        facts: ["Project Alpha due October 2026", "Widget torque is 185 ft-lbs"],
        decisions: ["Evaluate Mem0 sidecar during Phase 2"],
        openItems: ["Write integration tests for read paths"],
        keyEntities: ["Aletheia", "Qdrant"],
        contradictions: [],
      },
    });

    expect(result.written).toBe(true);
    expect(typeof result.path).toBe("string");
    expect(result.path.length).toBeGreaterThan(0);
  });

  it("creates the output file at the returned path", async () => {
    const { flushToWorkspace } = await import("./workspace-flush.js");
    const workspace = tmpDir();

    const result = flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: randomUUID(),
      distillationNumber: 1,
      summary: "Testing that the file actually exists after flush.",
      extraction: {
        facts: ["File existence is verifiable after flush"],
        decisions: [],
        openItems: [],
        keyEntities: [],
        contradictions: [],
      },
    });

    expect(existsSync(result.path)).toBe(true);
  });

  it("writes facts and decisions in markdown format", async () => {
    const { flushToWorkspace } = await import("./workspace-flush.js");
    const workspace = tmpDir();

    const result = flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: randomUUID(),
      distillationNumber: 1,
      summary: "Memory pipeline review session.",
      extraction: {
        facts: ["Neo4j graph relationships add value for concept clustering"],
        decisions: ["Fix Neo4j rather than remove it from the stack"],
        openItems: ["Run traffic trace to verify /add route usage"],
        keyEntities: ["Neo4j", "Qdrant"],
        contradictions: [],
      },
    });

    const content = readFileSync(result.path, "utf-8");
    expect(content).toContain("Neo4j graph relationships add value");
    expect(content).toContain("Fix Neo4j rather than remove it");
    expect(content).toContain("#### Key Facts");
    expect(content).toContain("#### Decisions");
  });

  it("creates memory subdirectory inside workspace", async () => {
    const { flushToWorkspace } = await import("./workspace-flush.js");
    const workspace = tmpDir();

    const result = flushToWorkspace({
      workspace,
      nousId: "chiron",
      sessionId: randomUUID(),
      distillationNumber: 1,
      summary: "Health and scheduling session.",
      extraction: {
        facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [],
      },
    });

    expect(result.path).toContain(join(workspace, "memory"));
    expect(existsSync(join(workspace, "memory"))).toBe(true);
  });

  it("returns written=false with an error message when workspace path is invalid", async () => {
    const { flushToWorkspace } = await import("./workspace-flush.js");

    // Use a path under an existing non-directory file to force mkdirSync to fail immediately.
    // Avoid /proc/ — writes to the procfs can block on Linux rather than failing fast.
    const tmpFile = join(tmpdir(), `pipeline-integration-notadir-${randomUUID().slice(0, 8)}`);
    const { writeFileSync } = await import("node:fs");
    writeFileSync(tmpFile, "not a directory");

    const result = flushToWorkspace({
      workspace: tmpFile, // workspace points to a regular file — memory subdir creation will fail
      nousId: "syn",
      sessionId: randomUUID(),
      distillationNumber: 1,
      summary: "Will fail because workspace is a file, not a directory.",
      extraction: {
        facts: [], decisions: [], openItems: [], keyEntities: [], contradictions: [],
      },
    });

    // Clean up temp file
    try { rmSync(tmpFile); } catch { /* ignore */ }

    expect(result.written).toBe(false);
    expect(result.error).toBeDefined();
  });
});

// --- Write Path 4: Finalize stage wiring (extractTurnFacts called via pipeline) ---
// Verified via static inspection of finalize.ts source: it imports extractTurnFacts from
// ../../turn-facts.js and calls it when memoryTarget is present and outcome.text.length > 150.
// Dynamic wiring is tested in finalize.test.ts (nous/pipeline/stages/) which uses vi.mock
// at the correct relative paths. The tests below confirm the integration contract: the shape
// of extractTurnFacts output matches what finalize.ts expects.

describe("write path: finalize stage wiring verification (extractTurnFacts)", () => {
  const finalizePath = join(
    process.cwd(),
    "src/nous/pipeline/stages/finalize.ts",
  );

  it("finalize.ts source imports extractTurnFacts (static wiring verification)", () => {
    // Reads finalize.ts source to confirm the import and call site exist.
    // Guards against accidental removal of the extractTurnFacts wiring.
    const finalizeSrc = readFileSync(finalizePath, "utf-8");

    expect(finalizeSrc).toMatch(/import.*extractTurnFacts.*from.*turn-facts/);
    expect(finalizeSrc).toMatch(/extractTurnFacts\(services\.router/);
  });

  it("finalize.ts invokes extractTurnFacts only when memoryTarget is set (static check)", () => {
    // Confirm the guard condition: if (services.memoryTarget && outcome.text.length > 150)
    const finalizeSrc = readFileSync(finalizePath, "utf-8");

    expect(finalizeSrc).toContain("services.memoryTarget");
    expect(finalizeSrc).toContain("outcome.text.length");
    expect(finalizeSrc).toContain("extractTurnFacts");
  });

  it("finalize.ts wires extracted facts to /add_batch sidecar endpoint (static check)", () => {
    // Confirm fetch is called with /add_batch after extractTurnFacts returns facts
    const finalizeSrc = readFileSync(finalizePath, "utf-8");

    expect(finalizeSrc).toContain("/add_batch");
    expect(finalizeSrc).toContain("agent_id");
    expect(finalizeSrc).toContain(`source: "turn"`);
  });
});
