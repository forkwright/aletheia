// Tests for CodebaseMap — structured codebase analysis (ENG-10)
import { describe, it, expect, beforeEach, afterEach } from "vitest";
import { mkdirSync, writeFileSync, existsSync, readFileSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import {
  detectLanguage,
  extractTsImports,
  extractTsExports,
  extractPyImports,
  extractCsImports,
  extractImports,
  extractExports,
  scanDirectory,
  groupIntoModules,
  detectLayers,
  detectConventions,
  generateCodebaseMap,
  writeCodebaseMapFile,
} from "./codebase-map.js";
import { ensureProjectDir, getProjectDir } from "./project-files.js";
import {
  PLANNING_V20_DDL, PLANNING_V21_MIGRATION, PLANNING_V22_MIGRATION,
  PLANNING_V23_MIGRATION, PLANNING_V24_MIGRATION, PLANNING_V25_MIGRATION,
  PLANNING_V26_MIGRATION, PLANNING_V27_MIGRATION,
} from "./schema.js";

function createTempWorkspace(): string {
  const dir = join(tmpdir(), `dianoia-codebase-test-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function writeFile(dir: string, path: string, content: string): void {
  const fullPath = join(dir, path);
  mkdirSync(join(fullPath, ".."), { recursive: true });
  writeFileSync(fullPath, content, "utf-8");
}

describe("CodebaseMap", () => {
  let workspace: string;

  beforeEach(() => {
    workspace = createTempWorkspace();
  });

  afterEach(() => {
    try { rmSync(workspace, { recursive: true, force: true }); } catch { /* ignore */ }
  });

  describe("detectLanguage", () => {
    it("detects TypeScript", () => {
      expect(detectLanguage("foo.ts")).toBe("typescript");
      expect(detectLanguage("foo.tsx")).toBe("typescript");
    });
    it("detects JavaScript", () => {
      expect(detectLanguage("foo.js")).toBe("javascript");
      expect(detectLanguage("foo.mjs")).toBe("javascript");
    });
    it("detects Python", () => {
      expect(detectLanguage("foo.py")).toBe("python");
    });
    it("detects C#", () => {
      expect(detectLanguage("foo.cs")).toBe("csharp");
    });
    it("returns unknown for unsupported", () => {
      expect(detectLanguage("foo.rs")).toBe("unknown");
      expect(detectLanguage("foo.md")).toBe("unknown");
    });
  });

  describe("extractTsImports", () => {
    it("extracts static imports", () => {
      const content = `import { foo } from "./foo.js";\nimport bar from "bar";`;
      const imports = extractTsImports(content);
      expect(imports).toContain("./foo.js");
      expect(imports).toContain("bar");
    });
    it("extracts type imports", () => {
      const content = `import type { Foo } from "./types.js";`;
      const imports = extractTsImports(content);
      expect(imports).toContain("./types.js");
    });
    it("extracts require calls", () => {
      const content = `const fs = require("node:fs");`;
      const imports = extractTsImports(content);
      expect(imports).toContain("node:fs");
    });
    it("extracts dynamic imports", () => {
      const content = `const mod = await import("./dynamic.js");`;
      const imports = extractTsImports(content);
      expect(imports).toContain("./dynamic.js");
    });
  });

  describe("extractTsExports", () => {
    it("extracts named exports", () => {
      const content = `export function foo() {}\nexport class Bar {}\nexport const baz = 1;`;
      const exports = extractTsExports(content);
      expect(exports).toContain("foo");
      expect(exports).toContain("Bar");
      expect(exports).toContain("baz");
    });
    it("extracts re-exports", () => {
      const content = `export { foo, bar as baz } from "./other.js";`;
      const exports = extractTsExports(content);
      expect(exports).toContain("baz");
    });
    it("extracts type exports", () => {
      const content = `export type Foo = string;\nexport interface Bar {}`;
      const exports = extractTsExports(content);
      expect(exports).toContain("Foo");
      expect(exports).toContain("Bar");
    });
  });

  describe("extractPyImports", () => {
    it("extracts simple imports", () => {
      const imports = extractPyImports("import os\nimport sys");
      expect(imports).toContain("os");
      expect(imports).toContain("sys");
    });
    it("extracts from imports", () => {
      const imports = extractPyImports("from pathlib import Path");
      expect(imports).toContain("pathlib");
    });
  });

  describe("extractCsImports", () => {
    it("extracts using statements", () => {
      const imports = extractCsImports("using System;\nusing System.Collections.Generic;");
      expect(imports).toContain("System");
      expect(imports).toContain("System.Collections.Generic");
    });
  });

  describe("scanDirectory", () => {
    it("finds source files recursively", () => {
      writeFile(workspace, "src/index.ts", 'export const x = 1;');
      writeFile(workspace, "src/utils/helper.ts", 'export function help() {}');
      writeFile(workspace, "README.md", "# Hello"); // Should be skipped

      const files = scanDirectory(workspace);
      expect(files.length).toBe(2);
      expect(files.some(f => f.path === "src/index.ts")).toBe(true);
      expect(files.some(f => f.path === "src/utils/helper.ts")).toBe(true);
    });

    it("skips node_modules and .git", () => {
      writeFile(workspace, "src/index.ts", 'export const x = 1;');
      writeFile(workspace, "node_modules/foo/index.js", 'module.exports = {}');
      writeFile(workspace, ".git/config", 'stuff');

      const files = scanDirectory(workspace);
      expect(files.length).toBe(1);
    });

    it("respects maxFiles limit", () => {
      for (let i = 0; i < 10; i++) {
        writeFile(workspace, `src/file${i}.ts`, `export const x${i} = ${i};`);
      }

      const files = scanDirectory(workspace, { maxFiles: 5 });
      expect(files.length).toBe(5);
    });

    it("extracts imports and exports from scanned files", () => {
      writeFile(workspace, "src/main.ts", 'import { foo } from "./foo.js";\nexport function main() {}');

      const files = scanDirectory(workspace);
      expect(files[0]!.imports).toContain("./foo.js");
      expect(files[0]!.exports).toContain("main");
    });
  });

  describe("groupIntoModules", () => {
    it("groups files by directory", () => {
      writeFile(workspace, "src/a.ts", "export const a = 1;");
      writeFile(workspace, "src/b.ts", "export const b = 2;");
      writeFile(workspace, "lib/c.ts", "export const c = 3;");

      const files = scanDirectory(workspace);
      const modules = groupIntoModules(files, workspace);

      expect(modules.length).toBe(2);
      const srcMod = modules.find(m => m.path === "src");
      expect(srcMod).toBeDefined();
      expect(srcMod!.files.length).toBe(2);
    });
  });

  describe("detectLayers", () => {
    it("detects architectural layers from module names", () => {
      const modules = [
        { path: "src/routes", language: "typescript" as const, files: ["a.ts"], totalLines: 100, dependsOn: [], usedBy: [] },
        { path: "src/services", language: "typescript" as const, files: ["b.ts"], totalLines: 200, dependsOn: [], usedBy: [] },
        { path: "src/store", language: "typescript" as const, files: ["c.ts"], totalLines: 150, dependsOn: [], usedBy: [] },
        { path: "src/components", language: "typescript" as const, files: ["d.ts"], totalLines: 300, dependsOn: [], usedBy: [] },
      ];

      const layers = detectLayers(modules);
      expect(layers.some(l => l.name === "API/Routes")).toBe(true);
      expect(layers.some(l => l.name === "Services/Business Logic")).toBe(true);
      expect(layers.some(l => l.name === "UI/Components")).toBe(true);
    });
  });

  describe("generateCodebaseMap", () => {
    it("generates complete map from workspace", () => {
      writeFile(workspace, "src/index.ts", 'import { foo } from "./foo.js";\nexport function main() {}');
      writeFile(workspace, "src/foo.ts", 'export function foo() { return 42; }');
      writeFile(workspace, "tests/main.test.ts", 'import { main } from "../src/index.js";');

      const map = generateCodebaseMap(workspace);

      expect(map.totalFiles).toBe(3);
      expect(map.languages.typescript).toBe(3);
      expect(map.modules.length).toBeGreaterThan(0);
      expect(map.generatedAt).toBeTruthy();
    });

    it("returns empty map for empty workspace", () => {
      const map = generateCodebaseMap(workspace);
      expect(map.totalFiles).toBe(0);
    });
  });

  describe("writeCodebaseMapFile", () => {
    it("writes CODEBASE.md for a project", () => {
      writeFile(workspace, "src/index.ts", 'export const x = 1;');
      const map = generateCodebaseMap(workspace);

      const projectId = "proj_test123";
      const projectDirValue = join(workspace, ".dianoia", "projects", projectId);
      ensureProjectDir(projectDirValue);
      writeCodebaseMapFile(projectDirValue, map);

      const filePath = join(getProjectDir(projectDirValue), "CODEBASE.md");
      expect(existsSync(filePath)).toBe(true);

      const content = readFileSync(filePath, "utf-8");
      expect(content).toContain("# Codebase Map");
      expect(content).toContain("typescript");
    });
  });
});
