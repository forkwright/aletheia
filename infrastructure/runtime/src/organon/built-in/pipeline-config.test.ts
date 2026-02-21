import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, rmSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createPipelineConfigTool } from "./pipeline-config.js";
import { clearPipelineConfigCache } from "../../nous/pipeline-config.js";
import type { ToolContext } from "../registry.js";

function makeContext(workspace: string): ToolContext {
  return { nousId: "test", sessionId: "ses_test", workspace };
}

describe("pipeline_config tool", () => {
  let tmpDir: string;
  let tool: ReturnType<typeof createPipelineConfigTool>;

  beforeEach(() => {
    tmpDir = join(tmpdir(), `pipeline-tool-test-${Date.now()}`);
    mkdirSync(tmpDir, { recursive: true });
    clearPipelineConfigCache();
    tool = createPipelineConfigTool();
  });

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true });
    clearPipelineConfigCache();
  });

  it("reads defaults when no file exists", async () => {
    const result = JSON.parse(await tool.execute({ action: "read" }, makeContext(tmpDir)));
    expect(result.source).toBe("defaults");
    expect(result.config.recall.limit).toBe(8);
  });

  it("writes config to pipeline.json", async () => {
    const result = JSON.parse(await tool.execute(
      { action: "write", config: { recall: { limit: 15 } } },
      makeContext(tmpDir),
    ));
    expect(result.config.recall.limit).toBe(15);
    expect(result.source).toBe("workspace");
    expect(existsSync(join(tmpDir, "pipeline.json"))).toBe(true);
  });

  it("merges partial writes", async () => {
    await tool.execute({ action: "write", config: { recall: { limit: 12 } } }, makeContext(tmpDir));
    const result = JSON.parse(await tool.execute(
      { action: "write", config: { tools: { expiryTurns: 10 } } },
      makeContext(tmpDir),
    ));
    expect(result.config.recall.limit).toBe(12);
    expect(result.config.tools.expiryTurns).toBe(10);
  });

  it("resets to defaults", async () => {
    writeFileSync(join(tmpDir, "pipeline.json"), JSON.stringify({ recall: { limit: 20 } }));
    const result = JSON.parse(await tool.execute({ action: "reset" }, makeContext(tmpDir)));
    expect(result.source).toBe("defaults");
    expect(result.config.recall.limit).toBe(8);
    expect(existsSync(join(tmpDir, "pipeline.json"))).toBe(false);
  });

  it("rejects invalid values", async () => {
    const result = JSON.parse(await tool.execute(
      { action: "write", config: { recall: { limit: 999 } } },
      makeContext(tmpDir),
    ));
    expect(result.error).toContain("Validation failed");
  });

  it("returns error for write without config", async () => {
    const result = JSON.parse(await tool.execute({ action: "write" }, makeContext(tmpDir)));
    expect(result.error).toContain("config object required");
  });
});
