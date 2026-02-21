// self_evaluate tool tests
import { beforeEach, describe, expect, it } from "vitest";
import { SessionStore } from "../../mneme/store.js";
import { CompetenceModel } from "../../nous/competence.js";
import { createSelfEvaluateTool } from "./self-evaluate.js";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

const ctx = { nousId: "syn", sessionId: "ses_1", workspace: "/tmp" };

describe("self_evaluate", () => {
  let store: SessionStore;
  let tmpDir: string;

  beforeEach(() => {
    store = new SessionStore(":memory:");
    tmpDir = mkdtempSync(join(tmpdir(), "aletheia-eval-"));
  });

  it("returns evaluation without competence model", async () => {
    const tool = createSelfEvaluateTool(store);
    const result = JSON.parse(await tool.execute({}, ctx));

    expect(result.nousId).toBe("syn");
    expect(result.competence.note).toContain("not available");
    expect(result.calibration.note).toContain("not available");
    expect(result.recentActivity).toBeDefined();
    expect(result.recommendations).toBeDefined();
  });

  it("returns competence data when model has data", async () => {
    const competence = new CompetenceModel(tmpDir);
    competence.recordSuccess("syn", "code");
    competence.recordSuccess("syn", "code");

    const tool = createSelfEvaluateTool(store, competence);
    const result = JSON.parse(await tool.execute({}, ctx));

    expect(result.overallScore).toBeGreaterThan(0);
    expect(result.domains.code).toBeDefined();
    expect(result.domains.code.successes).toBe(2);
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("generates recommendation for low-scoring domain", async () => {
    const competence = new CompetenceModel(tmpDir);
    // Drive score below 0.35 with corrections
    for (let i = 0; i < 5; i++) competence.recordCorrection("syn", "health");

    const tool = createSelfEvaluateTool(store, competence);
    const result = JSON.parse(await tool.execute({}, ctx));

    const healthRec = result.recommendations.find((r: string) => r.includes("health"));
    expect(healthRec).toContain("delegating");
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("generates recommendation when corrections exceed successes", async () => {
    const competence = new CompetenceModel(tmpDir);
    competence.recordCorrection("syn", "scheduling");
    competence.recordCorrection("syn", "scheduling");
    competence.recordCorrection("syn", "scheduling");
    competence.recordSuccess("syn", "scheduling");

    const tool = createSelfEvaluateTool(store, competence);
    const result = JSON.parse(await tool.execute({}, ctx));

    const schedRec = result.recommendations.find((r: string) => r.includes("scheduling"));
    expect(schedRec).toContain("corrections");
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("counts recent sessions within period", async () => {
    store.createSession("syn", "recent-test");
    const tool = createSelfEvaluateTool(store);
    const result = JSON.parse(await tool.execute({ days: 1 }, ctx));

    expect(result.recentActivity.recentSessions).toBeGreaterThanOrEqual(1);
  });

  it("reports no activity for inactive agent", async () => {
    const tool = createSelfEvaluateTool(store);
    const result = JSON.parse(await tool.execute({ days: 0 }, ctx));

    expect(result.recommendations).toContainEqual(
      expect.stringContaining("No recent activity"),
    );
  });
});
