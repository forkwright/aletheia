// Competence model tests
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { CompetenceModel } from "./competence.js";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

let tmpDir: string;
let model: CompetenceModel;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "competence-"));
  model = new CompetenceModel(tmpDir);
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("CompetenceModel", () => {
  it("starts with default score 0.5", () => {
    expect(model.getScore("syn", "scheduling")).toBe(0.5);
  });

  it("recordSuccess increases score by 0.02", () => {
    model.recordSuccess("syn", "health");
    expect(model.getScore("syn", "health")).toBeCloseTo(0.52);
  });

  it("recordCorrection decreases score by 0.05", () => {
    model.recordCorrection("syn", "health");
    expect(model.getScore("syn", "health")).toBeCloseTo(0.45);
  });

  it("recordDisagreement decreases score by 0.01", () => {
    model.recordDisagreement("syn", "health");
    expect(model.getScore("syn", "health")).toBeCloseTo(0.49);
  });

  it("score floors at 0.1", () => {
    for (let i = 0; i < 20; i++) model.recordCorrection("syn", "health");
    expect(model.getScore("syn", "health")).toBeCloseTo(0.1);
  });

  it("score caps at 0.95", () => {
    for (let i = 0; i < 50; i++) model.recordSuccess("syn", "health");
    expect(model.getScore("syn", "health")).toBeCloseTo(0.95);
  });

  it("getAgentCompetence returns agent data", () => {
    model.recordSuccess("syn", "health");
    const agent = model.getAgentCompetence("syn");
    expect(agent).not.toBeNull();
    expect(agent!.nousId).toBe("syn");
    expect(agent!.domains["health"]).toBeDefined();
  });

  it("getAgentCompetence returns null for unknown agent", () => {
    expect(model.getAgentCompetence("unknown")).toBeNull();
  });

  it("bestAgentForDomain finds highest scoring agent", () => {
    model.recordSuccess("syn", "health");
    model.recordSuccess("syn", "health");
    model.recordSuccess("chiron", "health");
    model.recordSuccess("chiron", "health");
    model.recordSuccess("chiron", "health");

    const best = model.bestAgentForDomain("health");
    expect(best!.nousId).toBe("chiron");
    expect(best!.score).toBeGreaterThan(model.getScore("syn", "health"));
  });

  it("bestAgentForDomain respects exclude list", () => {
    model.recordSuccess("chiron", "health");
    model.recordSuccess("chiron", "health");
    model.recordSuccess("syn", "health");

    const best = model.bestAgentForDomain("health", ["chiron"]);
    expect(best!.nousId).toBe("syn");
  });

  it("bestAgentForDomain returns null when no agent has domain", () => {
    expect(model.bestAgentForDomain("unknown")).toBeNull();
  });

  it("recalculates overall score as mean of domains", () => {
    model.recordSuccess("syn", "health");   // 0.52
    model.recordCorrection("syn", "code");  // 0.45
    const agent = model.getAgentCompetence("syn")!;
    expect(agent.overallScore).toBeCloseTo((0.52 + 0.45) / 2);
  });

  it("persists and reloads from file", () => {
    model.recordSuccess("syn", "health");
    model.recordSuccess("syn", "health");

    const model2 = new CompetenceModel(tmpDir);
    expect(model2.getScore("syn", "health")).toBeCloseTo(0.54);
  });

  it("toJSON returns internal data", () => {
    model.recordSuccess("syn", "test");
    const json = model.toJSON();
    expect(json["syn"]).toBeDefined();
  });

  it("tracks correction/success/disagreement counts", () => {
    model.recordSuccess("syn", "d");
    model.recordSuccess("syn", "d");
    model.recordCorrection("syn", "d");
    model.recordDisagreement("syn", "d");

    const domain = model.getAgentCompetence("syn")!.domains["d"]!;
    expect(domain.successes).toBe(2);
    expect(domain.corrections).toBe(1);
    expect(domain.disagreements).toBe(1);
  });
});
