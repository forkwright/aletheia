// Built-in tool tests — tests tool definitions and execute functions using temp workspace
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdtempSync, writeFileSync, mkdirSync, rmSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import type { ToolContext } from "../registry.js";

let workspace: string;
let ctx: ToolContext;

beforeEach(() => {
  workspace = mkdtempSync(join(tmpdir(), "tools-"));
  ctx = { nousId: "syn", sessionId: "ses_1", workspace };
});

afterEach(() => {
  rmSync(workspace, { recursive: true, force: true });
});

describe("readTool", () => {
  it("reads a file within workspace", async () => {
    writeFileSync(join(workspace, "test.txt"), "hello world");
    const { readTool } = await import("./read.js");
    const result = await readTool.execute({ path: "test.txt" }, ctx);
    expect(result).toContain("hello world");
  });

  it("resolves paths outside workspace without restriction", async () => {
    const { readTool } = await import("./read.js");
    // Should not throw — path restrictions removed
    const result = await readTool.execute({ path: "/etc/hostname" }, ctx);
    expect(typeof result).toBe("string");
  });

  it("has valid tool definition", async () => {
    const { readTool } = await import("./read.js");
    expect(readTool.definition.name).toBe("read");
    expect(readTool.definition.input_schema).toBeDefined();
  });
});

describe("writeTool", () => {
  it("writes a file within workspace", async () => {
    const { writeTool } = await import("./write.js");
    const result = await writeTool.execute(
      { path: "out.txt", content: "test content" },
      ctx,
    );
    expect(result).toContain("out.txt");
    expect(readFileSync(join(workspace, "out.txt"), "utf-8")).toBe("test content");
  });

  it("creates parent directories", async () => {
    const { writeTool } = await import("./write.js");
    await writeTool.execute(
      { path: "sub/dir/file.txt", content: "deep" },
      ctx,
    );
    expect(readFileSync(join(workspace, "sub/dir/file.txt"), "utf-8")).toBe("deep");
  });

  it("resolves paths outside workspace without restriction", async () => {
    const { writeTool } = await import("./write.js");
    // Should not throw — path restrictions removed
    const result = await writeTool.execute(
      { path: "/tmp/aletheia-test-write.txt", content: "test" },
      ctx,
    );
    expect(result).toContain("/tmp/aletheia-test-write.txt");
  });

  it("has valid tool definition", async () => {
    const { writeTool } = await import("./write.js");
    expect(writeTool.definition.name).toBe("write");
  });
});

describe("editTool", () => {
  it("replaces text in a file", async () => {
    writeFileSync(join(workspace, "edit.txt"), "hello world");
    const { editTool } = await import("./edit.js");
    const result = await editTool.execute(
      { path: "edit.txt", old_text: "world", new_text: "vitest" },
      ctx,
    );
    expect(readFileSync(join(workspace, "edit.txt"), "utf-8")).toContain("vitest");
  });

  it("has valid tool definition", async () => {
    const { editTool } = await import("./edit.js");
    expect(editTool.definition.name).toBe("edit");
  });
});

describe("lsTool", () => {
  it("lists files in workspace", async () => {
    writeFileSync(join(workspace, "a.txt"), "");
    writeFileSync(join(workspace, "b.txt"), "");
    const { lsTool } = await import("./ls.js");
    const result = await lsTool.execute({ path: "." }, ctx);
    expect(result).toContain("a.txt");
    expect(result).toContain("b.txt");
  });

  it("has valid tool definition", async () => {
    const { lsTool } = await import("./ls.js");
    expect(lsTool.definition.name).toBe("ls");
  });
});

describe("grepTool", () => {
  it("searches file contents", async () => {
    writeFileSync(join(workspace, "data.txt"), "line1\ntarget line\nline3");
    const { grepTool } = await import("./grep.js");
    const result = await grepTool.execute(
      { pattern: "target", path: "." },
      ctx,
    );
    expect(result).toContain("target");
  });

  it("has valid tool definition", async () => {
    const { grepTool } = await import("./grep.js");
    expect(grepTool.definition.name).toBe("grep");
  });
});

describe("findTool", () => {
  it("finds files by name pattern", async () => {
    writeFileSync(join(workspace, "test.ts"), "");
    const { findTool } = await import("./find.js");
    const result = await findTool.execute(
      { pattern: "test", path: "." },
      ctx,
    );
    expect(result).toContain("test.ts");
  });

  it("has valid tool definition", async () => {
    const { findTool } = await import("./find.js");
    expect(findTool.definition.name).toBe("find");
  });
});

describe("execTool", () => {
  it("executes commands", async () => {
    const { execTool } = await import("./exec.js");
    const result = await execTool.execute({ command: "echo hello" }, ctx);
    expect(result).toContain("hello");
  });

  it("has valid tool definition", async () => {
    const { execTool } = await import("./exec.js");
    expect(execTool.definition.name).toBe("exec");
  });
});

describe("sessionStatusTool", () => {
  it("has valid tool definition", async () => {
    const { sessionStatusTool } = await import("./session-status.js");
    expect(sessionStatusTool.definition.name).toBe("session_status");
  });
});

describe("traceLookupTool", () => {
  it("has valid tool definition", async () => {
    const { traceLookupTool } = await import("./trace-lookup.js");
    expect(traceLookupTool.definition.name).toBe("trace_lookup");
  });
});
