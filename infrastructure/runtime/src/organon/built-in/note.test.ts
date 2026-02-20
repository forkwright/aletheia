import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { SessionStore } from "../../mneme/store.js";
import { createNoteTool } from "./note.js";
import type { ToolContext } from "../registry.js";

function makeContext(sessionId: string, nousId = "test-nous"): ToolContext {
  return {
    sessionId,
    nousId,
    sessionKey: "main",
    workspace: "/tmp",
    channel: "test",
  };
}

describe("note tool", () => {
  let tmpDir: string;
  let store: SessionStore;

  beforeEach(() => {
    tmpDir = mkdtempSync(join(tmpdir(), "note-test-"));
    store = new SessionStore(join(tmpDir, "test.db"));
  });

  afterEach(() => {
    store.close();
    rmSync(tmpDir, { recursive: true, force: true });
  });

  it("adds a note", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    const result = await tool.execute(
      { action: "add", content: "Remember: user prefers dark mode", category: "preference" },
      ctx,
    );

    expect(result).toContain("saved");
    expect(result).toContain("preference");
    expect(result).toContain("dark mode");
  });

  it("lists notes", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    await tool.execute({ action: "add", content: "Note 1", category: "task" }, ctx);
    await tool.execute({ action: "add", content: "Note 2", category: "decision" }, ctx);

    const result = await tool.execute({ action: "list" }, ctx);

    expect(result).toContain("Notes (2)");
    expect(result).toContain("Note 1");
    expect(result).toContain("Note 2");
    expect(result).toContain("[task]");
    expect(result).toContain("[decision]");
  });

  it("lists notes filtered by category", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    await tool.execute({ action: "add", content: "Task note", category: "task" }, ctx);
    await tool.execute({ action: "add", content: "Decision note", category: "decision" }, ctx);

    const result = await tool.execute({ action: "list", category: "task" }, ctx);

    expect(result).toContain("Task note");
    expect(result).not.toContain("Decision note");
  });

  it("deletes a note", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    await tool.execute({ action: "add", content: "To delete", category: "context" }, ctx);
    const listBefore = await tool.execute({ action: "list" }, ctx);
    const idMatch = listBefore.match(/#(\d+)/);
    const noteId = idMatch ? parseInt(idMatch[1]!, 10) : 0;

    const deleteResult = await tool.execute({ action: "delete", id: noteId }, ctx);
    expect(deleteResult).toContain("deleted");

    const listAfter = await tool.execute({ action: "list" }, ctx);
    expect(listAfter).toContain("No notes");
  });

  it("rejects empty content", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    const result = await tool.execute({ action: "add", content: "" }, ctx);
    expect(result).toContain("Error");
  });

  it("defaults to context category", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    await tool.execute({ action: "add", content: "No category specified" }, ctx);
    const result = await tool.execute({ action: "list" }, ctx);
    expect(result).toContain("[context]");
  });

  it("truncates long content", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    const longContent = "x".repeat(1000);
    await tool.execute({ action: "add", content: longContent }, ctx);
    const result = await tool.execute({ action: "list" }, ctx);
    // Content should be truncated to 500 chars
    expect(result.length).toBeLessThan(longContent.length);
  });

  it("returns empty list for no notes", async () => {
    const session = store.findOrCreateSession("test-nous", "main");
    const tool = createNoteTool(store);
    const ctx = makeContext(session.id);

    const result = await tool.execute({ action: "list" }, ctx);
    expect(result).toContain("No notes");
  });
});
