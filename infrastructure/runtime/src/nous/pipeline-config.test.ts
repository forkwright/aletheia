import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { loadPipelineConfig, clearPipelineConfigCache, PipelineConfigSchema } from "./pipeline-config.js";

describe("PipelineConfigSchema", () => {
  it("returns defaults for empty input", () => {
    const result = PipelineConfigSchema.parse({});
    expect(result.recall.limit).toBe(8);
    expect(result.recall.maxTokens).toBe(1500);
    expect(result.recall.minScore).toBe(0.75);
    expect(result.recall.sufficiencyThreshold).toBe(0.85);
    expect(result.recall.sufficiencyMinHits).toBe(3);
    expect(result.tools.expiryTurns).toBe(5);
    expect(result.notes.tokenCap).toBe(2000);
  });

  it("accepts partial overrides", () => {
    const result = PipelineConfigSchema.parse({ recall: { limit: 15 } });
    expect(result.recall.limit).toBe(15);
    expect(result.recall.maxTokens).toBe(1500);
  });

  it("clamps values to range", () => {
    expect(() => PipelineConfigSchema.parse({ recall: { limit: 0 } })).toThrow();
    expect(() => PipelineConfigSchema.parse({ recall: { limit: 100 } })).toThrow();
    expect(() => PipelineConfigSchema.parse({ recall: { minScore: 2 } })).toThrow();
  });
});

describe("loadPipelineConfig", () => {
  let tmpDir: string;

  beforeEach(() => {
    tmpDir = join(tmpdir(), `pipeline-config-test-${Date.now()}`);
    mkdirSync(tmpDir, { recursive: true });
    clearPipelineConfigCache();
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
    clearPipelineConfigCache();
  });

  it("returns defaults when no file exists", () => {
    const config = loadPipelineConfig(tmpDir);
    expect(config.recall.limit).toBe(8);
    expect(config.tools.expiryTurns).toBe(5);
  });

  it("loads config from pipeline.json", () => {
    writeFileSync(join(tmpDir, "pipeline.json"), JSON.stringify({
      recall: { limit: 20, minScore: 0.5 },
      tools: { expiryTurns: 10 },
    }));
    const config = loadPipelineConfig(tmpDir);
    expect(config.recall.limit).toBe(20);
    expect(config.recall.minScore).toBe(0.5);
    expect(config.recall.maxTokens).toBe(1500);
    expect(config.tools.expiryTurns).toBe(10);
  });

  it("returns defaults for invalid JSON", () => {
    writeFileSync(join(tmpDir, "pipeline.json"), "not json");
    const config = loadPipelineConfig(tmpDir);
    expect(config.recall.limit).toBe(8);
  });

  it("returns defaults for out-of-range values", () => {
    writeFileSync(join(tmpDir, "pipeline.json"), JSON.stringify({
      recall: { limit: 999 },
    }));
    const config = loadPipelineConfig(tmpDir);
    expect(config.recall.limit).toBe(8);
  });

  it("caches by mtime", () => {
    writeFileSync(join(tmpDir, "pipeline.json"), JSON.stringify({ recall: { limit: 12 } }));
    const a = loadPipelineConfig(tmpDir);
    const b = loadPipelineConfig(tmpDir);
    expect(a).toBe(b);
  });
});
