// Workspace flush tests — distillation memory file writer
import { describe, it, expect, afterEach } from "vitest";
import { mkdtempSync, rmSync, readFileSync, existsSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { flushToWorkspace } from "./workspace-flush.js";
import type { ExtractionResult } from "./extract.js";

const EMPTY_EXTRACTION: ExtractionResult = {
  facts: [],
  decisions: [],
  openItems: [],
  keyEntities: [],
  contradictions: [],
};

function makeExtraction(overrides: Partial<ExtractionResult> = {}): ExtractionResult {
  return { ...EMPTY_EXTRACTION, ...overrides };
}

const dirs: string[] = [];

function tmpDir(): string {
  const d = mkdtempSync(join(tmpdir(), "workspace-flush-test-"));
  dirs.push(d);
  return d;
}

afterEach(() => {
  for (const d of dirs.splice(0)) {
    try { rmSync(d, { recursive: true }); } catch { /* ignore */ }
  }
});

describe("flushToWorkspace", () => {
  it("creates memory directory if missing", () => {
    const workspace = tmpDir();
    const result = flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "Did some work today.",
      extraction: EMPTY_EXTRACTION,
    });

    expect(result.written).toBe(true);
    expect(existsSync(join(workspace, "memory"))).toBe(true);
    expect(existsSync(result.path)).toBe(true);
  });

  it("writes file header and summary on first write", () => {
    const workspace = tmpDir();
    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "First summary.",
      extraction: EMPTY_EXTRACTION,
    });

    const dateStr = new Date().toISOString().slice(0, 10);
    const content = readFileSync(join(workspace, "memory", `${dateStr}.md`), "utf-8");
    expect(content).toContain(`# Memory — ${dateStr}`);
    expect(content).toContain("First summary.");
    expect(content).toContain("Distillation #1");
  });

  it("appends to existing file without duplicating header", () => {
    const workspace = tmpDir();

    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "First summary.",
      extraction: EMPTY_EXTRACTION,
    });

    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 2,
      summary: "Second summary.",
      extraction: EMPTY_EXTRACTION,
    });

    const dateStr = new Date().toISOString().slice(0, 10);
    const content = readFileSync(join(workspace, "memory", `${dateStr}.md`), "utf-8");

    expect(content).toContain("First summary.");
    expect(content).toContain("Second summary.");
    expect(content).toContain("Distillation #1");
    expect(content).toContain("Distillation #2");

    // Header appears exactly once
    const headerCount = (content.match(/^# Memory/gm) ?? []).length;
    expect(headerCount).toBe(1);
  });

  it("writes extraction sections when data present", () => {
    const workspace = tmpDir();
    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "Summary with extraction.",
      extraction: makeExtraction({
        facts: ["fact one", "fact two"],
        decisions: ["use TypeScript"],
        openItems: ["write tests"],
      }),
    });

    const dateStr = new Date().toISOString().slice(0, 10);
    const content = readFileSync(join(workspace, "memory", `${dateStr}.md`), "utf-8");
    expect(content).toContain("**Facts:** 2");
    expect(content).toContain("**Decisions:** 1");
    expect(content).toContain("**Open Items:** 1");
    expect(content).toContain("- fact one");
    expect(content).toContain("#### Key Facts");
    expect(content).toContain("#### Decisions");
    expect(content).toContain("#### Open Items");
  });

  it("omits extraction sections when all arrays are empty", () => {
    const workspace = tmpDir();
    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "Just a summary.",
      extraction: EMPTY_EXTRACTION,
    });

    const dateStr = new Date().toISOString().slice(0, 10);
    const content = readFileSync(join(workspace, "memory", `${dateStr}.md`), "utf-8");
    expect(content).not.toContain("### Extracted");
    expect(content).not.toContain("#### Key Facts");
  });

  it("caps facts at 20 and adds overflow note", () => {
    const workspace = tmpDir();
    const facts = Array.from({ length: 25 }, (_, i) => `fact ${i + 1}`);
    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "Many facts.",
      extraction: makeExtraction({ facts }),
    });

    const dateStr = new Date().toISOString().slice(0, 10);
    const content = readFileSync(join(workspace, "memory", `${dateStr}.md`), "utf-8");
    expect(content).toContain("- fact 20");
    expect(content).not.toContain("- fact 21");
    expect(content).toContain("... and 5 more");
  });

  it("returns error result when write fails (bad path)", () => {
    // Use a path where memory dir cannot be created
    const result = flushToWorkspace({
      workspace: "/proc/nonexistent-for-test",
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "will fail",
      extraction: EMPTY_EXTRACTION,
    });

    expect(result.written).toBe(false);
    expect(result.error).toBeDefined();
  });

  it("includes contradictions section when present", () => {
    const workspace = tmpDir();
    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "Contradictory session.",
      extraction: makeExtraction({
        facts: ["fact"],
        contradictions: ["previously X, now Y"],
      }),
    });

    const dateStr = new Date().toISOString().slice(0, 10);
    const content = readFileSync(join(workspace, "memory", `${dateStr}.md`), "utf-8");
    expect(content).toContain("**Contradictions:** 1");
    expect(content).toContain("#### Contradictions");
    expect(content).toContain("- previously X, now Y");
  });

  it("uses existing memory dir without error", () => {
    const workspace = tmpDir();
    mkdirSync(join(workspace, "memory"), { recursive: true });

    const result = flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 1,
      summary: "Dir already exists.",
      extraction: EMPTY_EXTRACTION,
    });

    expect(result.written).toBe(true);
  });

  it("pre-populates file before second flush to verify no duplicate header", () => {
    const workspace = tmpDir();
    const dateStr = new Date().toISOString().slice(0, 10);
    const memDir = join(workspace, "memory");
    mkdirSync(memDir, { recursive: true });
    writeFileSync(join(memDir, `${dateStr}.md`), `# Memory — ${dateStr}\n\nexisting content\n`);

    flushToWorkspace({
      workspace,
      nousId: "syn",
      sessionId: "ses_abc123",
      distillationNumber: 3,
      summary: "Appended to existing.",
      extraction: EMPTY_EXTRACTION,
    });

    const content = readFileSync(join(memDir, `${dateStr}.md`), "utf-8");
    expect(content).toContain("existing content");
    expect(content).toContain("Appended to existing.");
    const headerCount = (content.match(/^# Memory/gm) ?? []).length;
    expect(headerCount).toBe(1);
  });
});
