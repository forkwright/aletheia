// CodebaseMap — structured analysis of stack, architecture, conventions (ENG-10)
//
// Consumes workspace index, produces structured documentation about the codebase.
// Language-aware import/export parsing, architectural relationship extraction.
// Refreshed on demand. Separate from the indexer (which is file-level).
//
// Output feeds into context packets so sub-agents understand the codebase
// they're working in without re-discovering it every time.

import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { join, extname, relative, dirname, basename } from "node:path";
import { createLogger } from "../koina/logger.js";
import { ensureProjectDir } from "./project-files.js";

// Re-use atomic write
import { writeFileSync, renameSync, unlinkSync } from "node:fs";

function atomicWriteFile(filePath: string, content: string): void {
  const tmpPath = `${filePath}.tmp`;
  try {
    writeFileSync(tmpPath, content, "utf-8");
    renameSync(tmpPath, filePath);
  } catch (error) {
    try { if (existsSync(tmpPath)) unlinkSync(tmpPath); } catch { /* ignore */ }
    throw error;
  }
}

const log = createLogger("dianoia:codebase-map");

/** Supported languages for import/export parsing */
export type Language = "typescript" | "javascript" | "python" | "csharp" | "unknown";

export interface FileInfo {
  path: string;
  language: Language;
  size: number;
  imports: string[];
  exports: string[];
}

export interface ModuleInfo {
  /** Module path (relative to workspace root) */
  path: string;
  /** Primary language */
  language: Language;
  /** Files in this module */
  files: string[];
  /** Total lines of code */
  totalLines: number;
  /** Dependencies (other modules this imports from) */
  dependsOn: string[];
  /** Dependents (other modules that import from this) */
  usedBy: string[];
}

export interface ArchitecturalLayer {
  name: string;
  pattern: string;
  modules: string[];
}

export interface CodebaseMapResult {
  /** Workspace root that was analyzed */
  workspaceRoot: string;
  /** When the map was generated */
  generatedAt: string;
  /** Languages detected with file counts */
  languages: Record<Language, number>;
  /** Total files analyzed */
  totalFiles: number;
  /** Total lines of code */
  totalLines: number;
  /** Module-level view */
  modules: ModuleInfo[];
  /** Detected architectural layers */
  layers: ArchitecturalLayer[];
  /** Detected conventions */
  conventions: Convention[];
  /** Detected concerns/issues */
  concerns: string[];
}

export interface Convention {
  /** What the convention is */
  name: string;
  /** How it manifests */
  description: string;
  /** Evidence (file patterns, examples) */
  evidence: string[];
}

/** File extensions to language mapping */
const EXT_TO_LANG: Record<string, Language> = {
  ".ts": "typescript",
  ".tsx": "typescript",
  ".js": "javascript",
  ".jsx": "javascript",
  ".mjs": "javascript",
  ".cjs": "javascript",
  ".py": "python",
  ".cs": "csharp",
};

/** Directories to skip */
const SKIP_DIRS = new Set([
  "node_modules", ".git", "dist", "build", "coverage", ".next",
  "__pycache__", ".venv", "venv", "bin", "obj", ".dianoia",
]);

/**
 * Detect language from file extension.
 */
export function detectLanguage(filePath: string): Language {
  const ext = extname(filePath).toLowerCase();
  return EXT_TO_LANG[ext] ?? "unknown";
}

/**
 * Extract imports from a TypeScript/JavaScript file.
 */
export function extractTsImports(content: string): string[] {
  const imports: string[] = [];

  // import ... from "..."
  const staticImports = content.matchAll(/import\s+(?:(?:type\s+)?(?:\{[^}]*\}|[\w*]+(?:\s*,\s*\{[^}]*\})?)\s+from\s+)?['"]([^'"]+)['"]/g);
  for (const match of staticImports) {
    if (match[1]) imports.push(match[1]);
  }

  // require("...")
  const requires = content.matchAll(/require\s*\(\s*['"]([^'"]+)['"]\s*\)/g);
  for (const match of requires) {
    if (match[1]) imports.push(match[1]);
  }

  // Dynamic import("...")
  const dynamicImports = content.matchAll(/import\s*\(\s*['"]([^'"]+)['"]\s*\)/g);
  for (const match of dynamicImports) {
    if (match[1]) imports.push(match[1]);
  }

  return imports;
}

/**
 * Extract exports from a TypeScript/JavaScript file.
 */
export function extractTsExports(content: string): string[] {
  const exports: string[] = [];

  // export function/class/const/let/var/type/interface/enum
  const namedExports = content.matchAll(/export\s+(?:default\s+)?(?:async\s+)?(?:function|class|const|let|var|type|interface|enum)\s+(\w+)/g);
  for (const match of namedExports) {
    if (match[1]) exports.push(match[1]);
  }

  // export { ... }
  const reExports = content.matchAll(/export\s*\{([^}]+)\}/g);
  for (const match of reExports) {
    if (match[1]) {
      const names = match[1].split(",").map((n) => n.trim().split(/\s+as\s+/).pop()?.trim()).filter(Boolean);
      exports.push(...(names as string[]));
    }
  }

  return [...new Set(exports)];
}

/**
 * Extract imports from a Python file.
 */
export function extractPyImports(content: string): string[] {
  const imports: string[] = [];

  // import X / import X as Y
  const simpleImports = content.matchAll(/^import\s+([\w.]+)/gm);
  for (const match of simpleImports) {
    if (match[1]) imports.push(match[1]);
  }

  // from X import ...
  const fromImports = content.matchAll(/^from\s+([\w.]+)\s+import/gm);
  for (const match of fromImports) {
    if (match[1]) imports.push(match[1]);
  }

  return imports;
}

/**
 * Extract imports from a C# file.
 */
export function extractCsImports(content: string): string[] {
  const imports: string[] = [];

  // using X;
  const usings = content.matchAll(/^using\s+(?:static\s+)?([\w.]+)\s*;/gm);
  for (const match of usings) {
    if (match[1]) imports.push(match[1]);
  }

  return imports;
}

/**
 * Extract imports based on language.
 */
export function extractImports(content: string, language: Language): string[] {
  switch (language) {
    case "typescript":
    case "javascript":
      return extractTsImports(content);
    case "python":
      return extractPyImports(content);
    case "csharp":
      return extractCsImports(content);
    default:
      return [];
  }
}

/**
 * Extract exports based on language.
 */
export function extractExports(content: string, language: Language): string[] {
  switch (language) {
    case "typescript":
    case "javascript":
      return extractTsExports(content);
    default:
      return [];
  }
}

/**
 * Scan a directory recursively for source files.
 */
export function scanDirectory(
  rootDir: string,
  opts?: { maxFiles?: number; maxDepth?: number },
): FileInfo[] {
  const files: FileInfo[] = [];
  const maxFiles = opts?.maxFiles ?? 5000;
  const maxDepth = opts?.maxDepth ?? 20;

  function walk(dir: string, depth: number): void {
    if (depth > maxDepth || files.length >= maxFiles) return;

    let entries: string[];
    try {
      entries = readdirSync(dir);
    } catch {
      return;
    }

    for (const entry of entries) {
      if (files.length >= maxFiles) break;
      if (SKIP_DIRS.has(entry)) continue;

      const fullPath = join(dir, entry);
      let stat;
      try {
        stat = statSync(fullPath);
      } catch {
        continue;
      }

      if (stat.isDirectory()) {
        walk(fullPath, depth + 1);
      } else if (stat.isFile()) {
        const language = detectLanguage(fullPath);
        if (language === "unknown") continue;

        let content: string;
        try {
          // Only read first 50KB for import/export extraction
          const buffer = Buffer.alloc(50_000);
          const fd = require("node:fs").openSync(fullPath, "r");
          const bytesRead = require("node:fs").readSync(fd, buffer, 0, 50_000, 0);
          require("node:fs").closeSync(fd);
          content = buffer.toString("utf-8", 0, bytesRead);
        } catch {
          continue;
        }

        files.push({
          path: relative(rootDir, fullPath),
          language,
          size: stat.size,
          imports: extractImports(content, language),
          exports: extractExports(content, language),
        });
      }
    }
  }

  walk(rootDir, 0);
  return files;
}

/**
 * Group files into modules (by directory).
 */
export function groupIntoModules(files: FileInfo[], rootDir: string): ModuleInfo[] {
  const moduleMap = new Map<string, FileInfo[]>();

  for (const file of files) {
    const moduleDir = dirname(file.path) || ".";
    const list = moduleMap.get(moduleDir) ?? [];
    list.push(file);
    moduleMap.set(moduleDir, list);
  }

  const modules: ModuleInfo[] = [];

  for (const [path, moduleFiles] of moduleMap) {
    // Detect primary language
    const langCounts = new Map<Language, number>();
    for (const f of moduleFiles) {
      langCounts.set(f.language, (langCounts.get(f.language) ?? 0) + 1);
    }
    const primaryLang = [...langCounts.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? "unknown";

    // Count lines
    let totalLines = 0;
    for (const f of moduleFiles) {
      try {
        const fullPath = join(rootDir, f.path);
        const content = readFileSync(fullPath, "utf-8");
        totalLines += content.split("\n").length;
      } catch {
        // Estimate from size
        totalLines += Math.ceil(f.size / 40);
      }
    }

    // Collect all imports that reference other modules
    const allImports = new Set<string>();
    for (const f of moduleFiles) {
      for (const imp of f.imports) {
        // Resolve relative imports to module paths
        if (imp.startsWith(".")) {
          const resolved = join(dirname(f.path), imp).replace(/\.\w+$/, "");
          const resolvedDir = dirname(resolved);
          if (resolvedDir !== path && moduleMap.has(resolvedDir)) {
            allImports.add(resolvedDir);
          }
        }
      }
    }

    modules.push({
      path,
      language: primaryLang,
      files: moduleFiles.map((f) => f.path),
      totalLines,
      dependsOn: [...allImports],
      usedBy: [], // Filled in second pass
    });
  }

  // Second pass: compute usedBy (reverse of dependsOn)
  for (const mod of modules) {
    for (const dep of mod.dependsOn) {
      const depMod = modules.find((m) => m.path === dep);
      if (depMod) {
        depMod.usedBy.push(mod.path);
      }
    }
  }

  return modules;
}

/**
 * Detect architectural layers from module structure.
 */
export function detectLayers(modules: ModuleInfo[]): ArchitecturalLayer[] {
  const layers: ArchitecturalLayer[] = [];

  const layerPatterns: Array<{ name: string; pattern: RegExp }> = [
    { name: "API/Routes", pattern: /\b(routes?|controllers?|api|endpoints?)\b/i },
    { name: "Services/Business Logic", pattern: /\b(services?|business|domain|core)\b/i },
    { name: "Data/Repository", pattern: /\b(data|repository|store|model|schema|migration)\b/i },
    { name: "Infrastructure", pattern: /\b(infra|infrastructure|config|setup)\b/i },
    { name: "UI/Components", pattern: /\b(components?|views?|pages?|ui|layout)\b/i },
    { name: "Tests", pattern: /\b(tests?|spec|__tests__)\b/i },
    { name: "Utilities", pattern: /\b(utils?|helpers?|lib|common|shared)\b/i },
  ];

  for (const { name, pattern } of layerPatterns) {
    const matching = modules.filter((m) => pattern.test(m.path));
    if (matching.length > 0) {
      layers.push({
        name,
        pattern: pattern.source,
        modules: matching.map((m) => m.path),
      });
    }
  }

  return layers;
}

/**
 * Detect coding conventions from the codebase.
 */
export function detectConventions(files: FileInfo[], _modules: ModuleInfo[]): Convention[] {
  const conventions: Convention[] = [];

  // Check for index files (barrel exports)
  const indexFiles = files.filter((f) => basename(f.path).match(/^index\.(ts|js|tsx|jsx)$/));
  if (indexFiles.length > 3) {
    conventions.push({
      name: "Barrel exports",
      description: "Modules use index files to re-export public API",
      evidence: indexFiles.slice(0, 5).map((f) => f.path),
    });
  }

  // Check for test co-location
  const testFiles = files.filter((f) => f.path.match(/\.(test|spec)\.(ts|js|tsx|jsx)$/));
  const sourceFiles = files.filter((f) => !f.path.match(/\.(test|spec)\./));
  if (testFiles.length > 0) {
    const colocated = testFiles.filter((t) => {
      const srcPath = t.path.replace(/\.(test|spec)\./, ".");
      return sourceFiles.some((s) => s.path === srcPath);
    });
    if (colocated.length > testFiles.length * 0.5) {
      conventions.push({
        name: "Co-located tests",
        description: "Test files sit next to their source files",
        evidence: colocated.slice(0, 5).map((f) => f.path),
      });
    }
  }

  // Check for TypeScript strict mode
  const tsFiles = files.filter((f) => f.language === "typescript");
  if (tsFiles.length > 0) {
    conventions.push({
      name: "TypeScript codebase",
      description: `${tsFiles.length} TypeScript files detected`,
      evidence: [`${tsFiles.length} .ts/.tsx files`],
    });
  }

  // Check for ESM vs CJS
  const esmImports = files.filter((f) =>
    f.imports.some((i) => i.endsWith(".js")) || f.path.endsWith(".mjs"),
  );
  if (esmImports.length > files.length * 0.3) {
    conventions.push({
      name: "ESM modules",
      description: "Project uses ES module imports with .js extensions",
      evidence: esmImports.slice(0, 3).map((f) => f.path),
    });
  }

  return conventions;
}

/**
 * Generate a full codebase map for a workspace.
 */
export function generateCodebaseMap(
  workspaceRoot: string,
  opts?: { maxFiles?: number; maxDepth?: number },
): CodebaseMapResult {
  const start = Date.now();
  const files = scanDirectory(workspaceRoot, opts);

  // Language breakdown
  const languages: Record<Language, number> = {
    typescript: 0,
    javascript: 0,
    python: 0,
    csharp: 0,
    unknown: 0,
  };
  for (const f of files) {
    languages[f.language]++;
  }

  const modules = groupIntoModules(files, workspaceRoot);
  const layers = detectLayers(modules);
  const conventions = detectConventions(files, modules);

  // Detect concerns
  const concerns: string[] = [];
  const largeMods = modules.filter((m) => m.totalLines > 2000);
  if (largeMods.length > 0) {
    concerns.push(`${largeMods.length} large modules (>2000 lines): ${largeMods.map((m) => m.path).join(", ")}`);
  }
  const circularDeps = modules.filter((m) => m.dependsOn.some((d) => {
    const depMod = modules.find((dm) => dm.path === d);
    return depMod?.dependsOn.includes(m.path);
  }));
  if (circularDeps.length > 0) {
    concerns.push(`Circular dependencies detected: ${circularDeps.map((m) => m.path).join(", ")}`);
  }

  const totalLines = modules.reduce((sum, m) => sum + m.totalLines, 0);
  const duration = Date.now() - start;
  log.info(`Codebase map generated: ${files.length} files, ${modules.length} modules, ${totalLines} lines in ${duration}ms`);

  return {
    workspaceRoot,
    generatedAt: new Date().toISOString(),
    languages,
    totalFiles: files.length,
    totalLines,
    modules,
    layers,
    conventions,
    concerns,
  };
}

/**
 * Write a CODEBASE.md file for a project from a codebase map.
 */
export function writeCodebaseMapFile(
  projectDirValue: string,
  map: CodebaseMapResult,
): void {
  const dir = ensureProjectDir(projectDirValue);
  const lines: string[] = [];

  lines.push("# Codebase Map", "");
  lines.push(`*Generated: ${map.generatedAt}*`, "");
  lines.push(`**Files:** ${map.totalFiles} | **Lines:** ${map.totalLines.toLocaleString()} | **Modules:** ${map.modules.length}`, "");

  // Languages
  lines.push("## Languages", "");
  lines.push("| Language | Files |");
  lines.push("|----------|-------|");
  for (const [lang, count] of Object.entries(map.languages)) {
    if (count > 0) {
      lines.push(`| ${lang} | ${count} |`);
    }
  }
  lines.push("");

  // Architecture
  if (map.layers.length > 0) {
    lines.push("## Architecture", "");
    for (const layer of map.layers) {
      lines.push(`### ${layer.name}`);
      for (const mod of layer.modules) {
        lines.push(`- ${mod}`);
      }
      lines.push("");
    }
  }

  // Conventions
  if (map.conventions.length > 0) {
    lines.push("## Conventions", "");
    for (const conv of map.conventions) {
      lines.push(`### ${conv.name}`, "");
      lines.push(conv.description, "");
      if (conv.evidence.length > 0) {
        lines.push("Evidence:");
        for (const e of conv.evidence) {
          lines.push(`- \`${e}\``);
        }
        lines.push("");
      }
    }
  }

  // Concerns
  if (map.concerns.length > 0) {
    lines.push("## Concerns", "");
    for (const c of map.concerns) {
      lines.push(`- ⚠️ ${c}`);
    }
    lines.push("");
  }

  // Module detail (top 20 by size)
  const topModules = [...map.modules].sort((a, b) => b.totalLines - a.totalLines).slice(0, 20);
  if (topModules.length > 0) {
    lines.push("## Top Modules (by size)", "");
    lines.push("| Module | Language | Files | Lines | Deps | Used By |");
    lines.push("|--------|----------|-------|-------|------|---------|");
    for (const mod of topModules) {
      lines.push(`| ${mod.path} | ${mod.language} | ${mod.files.length} | ${mod.totalLines} | ${mod.dependsOn.length} | ${mod.usedBy.length} |`);
    }
    lines.push("");
  }

  const filePath = join(dir, "CODEBASE.md");
  atomicWriteFile(filePath, lines.join("\n"));
  log.debug(`Wrote CODEBASE.md for ${projectDirValue}`);
}
