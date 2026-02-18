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

  it("blocks absolute paths outside workspace", () => {
    expect(() => safePath(ws, "/etc/hosts")).toThrow("Path traversal blocked");
  });

  it("blocks parent traversal escaping workspace", () => {
    expect(() => safePath(ws, "../etc/passwd")).toThrow("Path traversal blocked");
  });

  it("blocks paths outside workspace without allowedRoots", () => {
    expect(() => safePath(ws, "/tmp/data.txt")).toThrow("Path traversal blocked");
  });

  it("handles . (current dir)", () => {
    expect(safePath(ws, ".")).toBe(ws);
  });

  it("handles ./relative paths", () => {
    expect(safePath(ws, "./file.txt")).toBe("/home/agent/workspace/file.txt");
  });

  it("allows paths in allowedRoots", () => {
    expect(safePath(ws, "/mnt/ssd/data.txt", ["/mnt/ssd"])).toBe("/mnt/ssd/data.txt");
  });

  it("blocks paths not in allowedRoots", () => {
    expect(() => safePath(ws, "/etc/passwd", ["/mnt/ssd"])).toThrow("Path traversal blocked");
  });

  it("allows workspace root itself", () => {
    expect(safePath(ws, ws)).toBe(ws);
  });

  it("allows allowedRoot itself", () => {
    expect(safePath(ws, "/mnt/ssd", ["/mnt/ssd"])).toBe("/mnt/ssd");
  });
});
