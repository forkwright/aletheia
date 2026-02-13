// Safe path containment tests
import { describe, it, expect } from "vitest";
import { safePath } from "./safe-path.js";

describe("safePath", () => {
  const ws = "/home/agent/workspace";

  it("resolves relative paths within workspace", () => {
    expect(safePath(ws, "file.txt")).toBe("/home/agent/workspace/file.txt");
  });

  it("resolves nested paths", () => {
    expect(safePath(ws, "sub/dir/file.txt")).toBe("/home/agent/workspace/sub/dir/file.txt");
  });

  it("resolves absolute paths within workspace", () => {
    expect(safePath(ws, "/home/agent/workspace/file.txt")).toBe("/home/agent/workspace/file.txt");
  });

  it("throws on parent traversal (..)", () => {
    expect(() => safePath(ws, "../etc/passwd")).toThrow("Path outside workspace");
  });

  it("throws on deeply nested traversal", () => {
    expect(() => safePath(ws, "sub/../../etc/passwd")).toThrow("Path outside workspace");
  });

  it("throws on absolute path outside workspace", () => {
    expect(() => safePath(ws, "/etc/passwd")).toThrow("Path outside workspace");
  });

  it("handles . (current dir)", () => {
    expect(safePath(ws, ".")).toBe(ws);
  });

  it("handles ./relative paths", () => {
    expect(safePath(ws, "./file.txt")).toBe("/home/agent/workspace/file.txt");
  });
});
