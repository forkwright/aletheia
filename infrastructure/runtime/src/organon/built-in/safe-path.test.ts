// Path resolution tests
import { describe, it, expect } from "vitest";
import { safePath } from "./safe-path.js";

describe("safePath", () => {
  const ws = "/home/agent/workspace";

  it("resolves relative paths against workspace", () => {
    expect(safePath(ws, "file.txt")).toBe("/home/agent/workspace/file.txt");
  });

  it("resolves nested paths", () => {
    expect(safePath(ws, "sub/dir/file.txt")).toBe("/home/agent/workspace/sub/dir/file.txt");
  });

  it("resolves absolute paths as-is", () => {
    expect(safePath(ws, "/etc/hosts")).toBe("/etc/hosts");
  });

  it("resolves parent traversal to absolute path", () => {
    expect(safePath(ws, "../etc/passwd")).toBe("/home/agent/etc/passwd");
  });

  it("resolves paths outside workspace without restriction", () => {
    expect(safePath(ws, "/tmp/data.txt")).toBe("/tmp/data.txt");
  });

  it("handles . (current dir)", () => {
    expect(safePath(ws, ".")).toBe(ws);
  });

  it("handles ./relative paths", () => {
    expect(safePath(ws, "./file.txt")).toBe("/home/agent/workspace/file.txt");
  });

  it("ignores allowedRoots parameter", () => {
    expect(safePath(ws, "/mnt/ssd/data.txt", ["/mnt/ssd"])).toBe("/mnt/ssd/data.txt");
  });
});
