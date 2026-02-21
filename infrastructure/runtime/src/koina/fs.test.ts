// Filesystem utility tests
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { exists, readJson, readText, writeJson, writeText } from "./fs.js";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";

let tmpDir: string;

beforeEach(() => {
  tmpDir = mkdtempSync(join(tmpdir(), "fs-test-"));
});

afterEach(() => {
  rmSync(tmpDir, { recursive: true, force: true });
});

describe("readText", () => {
  it("reads file contents", () => {
    const path = join(tmpDir, "test.txt");
    writeText(path, "hello");
    expect(readText(path)).toBe("hello");
  });

  it("returns null for missing files", () => {
    expect(readText(join(tmpDir, "nope.txt"))).toBeNull();
  });
});

describe("readJson", () => {
  it("parses JSON files", () => {
    const path = join(tmpDir, "test.json");
    writeJson(path, { key: "value" });
    expect(readJson(path)).toEqual({ key: "value" });
  });

  it("returns null for invalid JSON", () => {
    const path = join(tmpDir, "bad.json");
    writeText(path, "not json");
    expect(readJson(path)).toBeNull();
  });

  it("returns null for missing files", () => {
    expect(readJson(join(tmpDir, "nope.json"))).toBeNull();
  });
});

describe("writeText", () => {
  it("creates parent directories", () => {
    const path = join(tmpDir, "sub", "dir", "file.txt");
    writeText(path, "deep");
    expect(readText(path)).toBe("deep");
  });
});

describe("writeJson", () => {
  it("pretty-prints with trailing newline", () => {
    const path = join(tmpDir, "out.json");
    writeJson(path, { a: 1 });
    const raw = readText(path)!;
    expect(raw).toContain('"a": 1');
    expect(raw.endsWith("\n")).toBe(true);
  });
});

describe("exists", () => {
  it("returns true for existing files", () => {
    const path = join(tmpDir, "exists.txt");
    writeText(path, "");
    expect(exists(path)).toBe(true);
  });

  it("returns false for missing files", () => {
    expect(exists(join(tmpDir, "nope"))).toBe(false);
  });
});
