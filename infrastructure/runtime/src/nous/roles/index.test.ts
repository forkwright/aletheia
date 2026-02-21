// Sub-agent role definitions and result parsing tests
import { describe, expect, it } from "vitest";
import {
  isValidRole,
  parseStructuredResult,
  type RoleName,
  ROLES,
} from "./index.js";

describe("ROLES", () => {
  const roleNames: RoleName[] = ["coder", "reviewer", "researcher", "explorer", "runner"];

  it("defines all five roles", () => {
    expect(Object.keys(ROLES)).toEqual(roleNames);
  });

  for (const name of roleNames) {
    describe(name, () => {
      it("has a model", () => {
        expect(ROLES[name].model).toMatch(/^anthropic\//);
      });

      it("has a non-empty system prompt", () => {
        expect(ROLES[name].systemPrompt.length).toBeGreaterThan(100);
      });

      it("has at least one tool", () => {
        expect(ROLES[name].tools.length).toBeGreaterThan(0);
      });

      it("has positive maxTurns", () => {
        expect(ROLES[name].maxTurns).toBeGreaterThan(0);
      });

      it("has positive maxTokenBudget", () => {
        expect(ROLES[name].maxTokenBudget).toBeGreaterThan(0);
      });

      it("has a description", () => {
        expect(ROLES[name].description.length).toBeGreaterThan(0);
      });

      it("system prompt mentions structured result contract", () => {
        expect(ROLES[name].systemPrompt).toContain("```json");
        expect(ROLES[name].systemPrompt).toContain('"status"');
        expect(ROLES[name].systemPrompt).toContain('"summary"');
      });

      it("system prompt mentions the role name", () => {
        expect(ROLES[name].systemPrompt).toContain(`"role": "${name}"`);
      });
    });
  }

  it("uses cheaper models for explorer and runner", () => {
    expect(ROLES["explorer"].model).toContain("haiku");
    expect(ROLES["runner"].model).toContain("haiku");
  });

  it("uses mid-tier models for coder, reviewer, researcher", () => {
    expect(ROLES["coder"].model).toContain("sonnet");
    expect(ROLES["reviewer"].model).toContain("sonnet");
    expect(ROLES["researcher"].model).toContain("sonnet");
  });

  it("explorer and runner have no write tools", () => {
    expect(ROLES["explorer"].tools).not.toContain("write");
    expect(ROLES["explorer"].tools).not.toContain("edit");
    expect(ROLES["runner"].tools).not.toContain("write");
    expect(ROLES["runner"].tools).not.toContain("edit");
  });

  it("coder has write and edit tools", () => {
    expect(ROLES["coder"].tools).toContain("write");
    expect(ROLES["coder"].tools).toContain("edit");
  });

  it("reviewer has no write tools", () => {
    expect(ROLES["reviewer"].tools).not.toContain("write");
    expect(ROLES["reviewer"].tools).not.toContain("edit");
  });
});

describe("isValidRole", () => {
  it("returns true for valid roles", () => {
    expect(isValidRole("coder")).toBe(true);
    expect(isValidRole("reviewer")).toBe(true);
    expect(isValidRole("researcher")).toBe(true);
    expect(isValidRole("explorer")).toBe(true);
    expect(isValidRole("runner")).toBe(true);
  });

  it("returns false for invalid roles", () => {
    expect(isValidRole("hacker")).toBe(false);
    expect(isValidRole("")).toBe(false);
    expect(isValidRole("CODER")).toBe(false);
  });
});

describe("parseStructuredResult", () => {
  it("parses a valid JSON result block", () => {
    const text = `I did the work. Here are my findings.

\`\`\`json
{
  "role": "explorer",
  "task": "find distillation triggers",
  "status": "success",
  "summary": "Found 3 trigger points in the pipeline.",
  "details": {"relevantFiles": [{"path": "src/distillation/pipeline.ts"}]},
  "confidence": 0.9
}
\`\`\``;

    const result = parseStructuredResult(text);
    expect(result).not.toBeNull();
    expect(result!.role).toBe("explorer");
    expect(result!.status).toBe("success");
    expect(result!.summary).toBe("Found 3 trigger points in the pipeline.");
    expect(result!.confidence).toBe(0.9);
  });

  it("uses the last JSON block when multiple exist", () => {
    const text = `Here's some code:

\`\`\`json
{"irrelevant": true}
\`\`\`

And the result:

\`\`\`json
{
  "role": "coder",
  "task": "add column",
  "status": "success",
  "summary": "Added the column.",
  "details": {},
  "confidence": 0.95
}
\`\`\``;

    const result = parseStructuredResult(text);
    expect(result).not.toBeNull();
    expect(result!.role).toBe("coder");
  });

  it("returns null when no JSON block exists", () => {
    expect(parseStructuredResult("Just some text with no JSON.")).toBeNull();
  });

  it("returns null for malformed JSON", () => {
    const text = `\`\`\`json
{broken json here}
\`\`\``;
    expect(parseStructuredResult(text)).toBeNull();
  });

  it("returns null when required fields are missing", () => {
    const text = `\`\`\`json
{"role": "coder", "task": "thing"}
\`\`\``;
    expect(parseStructuredResult(text)).toBeNull();
  });

  it("defaults missing optional fields", () => {
    const text = `\`\`\`json
{
  "status": "success",
  "summary": "Done."
}
\`\`\``;

    const result = parseStructuredResult(text);
    expect(result).not.toBeNull();
    expect(result!.role).toBe("unknown");
    expect(result!.task).toBe("");
    expect(result!.confidence).toBe(0.5);
    expect(result!.details).toEqual({});
  });

  it("preserves filesChanged array", () => {
    const text = `\`\`\`json
{
  "status": "success",
  "summary": "Changed files.",
  "filesChanged": ["src/a.ts", "src/b.ts"]
}
\`\`\``;

    const result = parseStructuredResult(text);
    expect(result!.filesChanged).toEqual(["src/a.ts", "src/b.ts"]);
  });

  it("preserves issues array", () => {
    const text = `\`\`\`json
{
  "status": "success",
  "summary": "Found issues.",
  "issues": [
    {"severity": "error", "location": "store.ts:42", "message": "null check missing"}
  ]
}
\`\`\``;

    const result = parseStructuredResult(text);
    expect(result!.issues).toHaveLength(1);
    expect(result!.issues![0].severity).toBe("error");
  });
});
