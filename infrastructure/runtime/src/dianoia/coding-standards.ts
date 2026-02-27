// CodingStandards — layered coding standard system (ENG-15)
//
// 4 layers of coding standards, each inheriting from the layer above:
// L0: Universal — self-documenting, no dead code, consistent formatting
// L1: Language/domain — TypeScript strict mode, Python PEP 8, C# naming
// L2: Project-specific — this project's patterns, file structure, conventions
// L3: User preferences — learned over time from corrections and code reviews
//
// Machine-readable (JSON), inheritable (L2 includes L1 includes L0),
// template-backed (defaults per language).

import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { getProjectDir, ensureProjectDir } from "./project-files.js";

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

const log = createLogger("dianoia:standards");

export type StandardLevel = 0 | 1 | 2 | 3;

export interface CodingRule {
  /** Unique rule ID (e.g., "L0-001", "L1-TS-003") */
  id: string;
  /** Which layer this rule belongs to */
  level: StandardLevel;
  /** Human-readable name */
  name: string;
  /** Description of the rule */
  description: string;
  /** How to check compliance (can be automated or manual) */
  check: "lint" | "type-check" | "review" | "manual";
  /** Severity: error blocks merge, warn is advisory */
  severity: "error" | "warn" | "info";
  /** Example of correct usage */
  goodExample?: string;
  /** Example of incorrect usage */
  badExample?: string;
  /** Whether this rule is enabled */
  enabled: boolean;
}

export interface StandardsLayer {
  level: StandardLevel;
  name: string;
  description: string;
  rules: CodingRule[];
}

export interface ProjectStandards {
  /** Project these standards apply to */
  projectId: string | null;
  /** All 4 layers */
  layers: StandardsLayer[];
  /** Computed: all enabled rules across all layers (L0 → L3) */
  effectiveRules: CodingRule[];
  /** When standards were last updated */
  updatedAt: string;
}

// --- L0: Universal standards (apply to all code) ---

const L0_RULES: CodingRule[] = [
  {
    id: "L0-001", level: 0, name: "Self-documenting names",
    description: "Variables, functions, and types should be named clearly enough that comments are rarely needed",
    check: "review", severity: "warn", enabled: true,
  },
  {
    id: "L0-002", level: 0, name: "No dead code",
    description: "Remove commented-out code, unused imports, unreachable branches",
    check: "lint", severity: "error", enabled: true,
  },
  {
    id: "L0-003", level: 0, name: "Consistent formatting",
    description: "Use project formatter (prettier, black, dotnet-format). No manual formatting debates.",
    check: "lint", severity: "error", enabled: true,
  },
  {
    id: "L0-004", level: 0, name: "Error handling",
    description: "No swallowed errors. Catch blocks must log, rethrow, or return meaningful error.",
    check: "review", severity: "error", enabled: true,
  },
  {
    id: "L0-005", level: 0, name: "Single responsibility",
    description: "Functions do one thing. Files have one purpose. Modules have clear boundaries.",
    check: "review", severity: "warn", enabled: true,
  },
  {
    id: "L0-006", level: 0, name: "Atomic commits",
    description: "Each commit does one logical thing. Conventional commit messages.",
    check: "manual", severity: "warn", enabled: true,
  },
  {
    id: "L0-007", level: 0, name: "No magic numbers/strings",
    description: "Named constants for non-obvious values. Config over hardcoding.",
    check: "review", severity: "warn", enabled: true,
  },
  {
    id: "L0-008", level: 0, name: "Fail fast",
    description: "Validate inputs early. Return errors before doing work.",
    check: "review", severity: "warn", enabled: true,
  },
];

// --- L1: Language-specific templates ---

const L1_TYPESCRIPT_RULES: CodingRule[] = [
  {
    id: "L1-TS-001", level: 1, name: "Strict TypeScript",
    description: "strict: true in tsconfig. No any unless explicitly justified.",
    check: "type-check", severity: "error", enabled: true,
  },
  {
    id: "L1-TS-002", level: 1, name: "Explicit return types",
    description: "Public functions and methods should have explicit return types",
    check: "lint", severity: "warn", enabled: true,
  },
  {
    id: "L1-TS-003", level: 1, name: "Prefer type imports",
    description: "Use 'import type' for type-only imports to improve tree-shaking",
    check: "lint", severity: "info", enabled: true,
  },
  {
    id: "L1-TS-004", level: 1, name: "ESM imports with .js extension",
    description: "Import paths end in .js for ESM compatibility (TypeScript resolves .ts → .js)",
    check: "type-check", severity: "error", enabled: true,
  },
  {
    id: "L1-TS-005", level: 1, name: "Zod for runtime validation",
    description: "Use Zod schemas for external data validation (API input, file parsing, config)",
    check: "review", severity: "warn", enabled: true,
  },
  {
    id: "L1-TS-006", level: 1, name: "Readonly by default",
    description: "Prefer const, readonly, and ReadonlyArray. Mutate only when necessary.",
    check: "review", severity: "info", enabled: true,
  },
];

const L1_PYTHON_RULES: CodingRule[] = [
  {
    id: "L1-PY-001", level: 1, name: "Type hints",
    description: "All function signatures should have type hints (PEP 484)",
    check: "lint", severity: "warn", enabled: true,
  },
  {
    id: "L1-PY-002", level: 1, name: "PEP 8 formatting",
    description: "Use black or ruff for consistent formatting",
    check: "lint", severity: "error", enabled: true,
  },
  {
    id: "L1-PY-003", level: 1, name: "Docstrings",
    description: "Public functions and classes should have docstrings (Google style)",
    check: "lint", severity: "warn", enabled: true,
  },
  {
    id: "L1-PY-004", level: 1, name: "No bare except",
    description: "Always catch specific exceptions. No 'except:' or 'except Exception:'.",
    check: "lint", severity: "error", enabled: true,
  },
];

const L1_CSHARP_RULES: CodingRule[] = [
  {
    id: "L1-CS-001", level: 1, name: "PascalCase public members",
    description: "Public methods, properties, and classes use PascalCase",
    check: "lint", severity: "error", enabled: true,
  },
  {
    id: "L1-CS-002", level: 1, name: "Nullable reference types",
    description: "Enable nullable reference types. Explicit null checks.",
    check: "type-check", severity: "error", enabled: true,
  },
  {
    id: "L1-CS-003", level: 1, name: "Async all the way",
    description: "Don't mix sync/async. Use async/await consistently.",
    check: "review", severity: "warn", enabled: true,
  },
];

/** Language templates */
const LANGUAGE_TEMPLATES: Record<string, CodingRule[]> = {
  typescript: L1_TYPESCRIPT_RULES,
  javascript: L1_TYPESCRIPT_RULES.filter((r) => !r.id.includes("TS-001")), // No strict TS for JS
  python: L1_PYTHON_RULES,
  csharp: L1_CSHARP_RULES,
};

/**
 * Get L1 rules for a specific language.
 */
export function getLanguageRules(language: string): CodingRule[] {
  return LANGUAGE_TEMPLATES[language.toLowerCase()] ?? [];
}

/**
 * Build the complete standards stack for a project.
 *
 * @param primaryLanguage - The main language of the project (for L1 template)
 * @param projectRules - Custom L2 rules specific to this project
 * @param userRules - L3 rules learned from user corrections
 */
export function buildStandards(opts: {
  projectId?: string;
  primaryLanguage: string;
  projectRules?: CodingRule[];
  userRules?: CodingRule[];
}): ProjectStandards {
  const l0: StandardsLayer = {
    level: 0,
    name: "Universal",
    description: "Apply to all code regardless of language or project",
    rules: L0_RULES,
  };

  const l1Rules = getLanguageRules(opts.primaryLanguage);
  const l1: StandardsLayer = {
    level: 1,
    name: `Language: ${opts.primaryLanguage}`,
    description: `${opts.primaryLanguage}-specific rules and conventions`,
    rules: l1Rules,
  };

  const l2: StandardsLayer = {
    level: 2,
    name: "Project",
    description: "Project-specific patterns and conventions",
    rules: opts.projectRules ?? [],
  };

  const l3: StandardsLayer = {
    level: 3,
    name: "User Preferences",
    description: "Rules learned from corrections and code reviews over time",
    rules: opts.userRules ?? [],
  };

  // Compute effective rules: all enabled rules from L0 → L3
  // Higher layers can override lower layers (same rule ID = higher layer wins)
  const ruleMap = new Map<string, CodingRule>();
  for (const layer of [l0, l1, l2, l3]) {
    for (const rule of layer.rules) {
      if (rule.enabled) {
        ruleMap.set(rule.id, rule);
      }
    }
  }

  return {
    projectId: opts.projectId ?? null,
    layers: [l0, l1, l2, l3],
    effectiveRules: [...ruleMap.values()],
    updatedAt: new Date().toISOString(),
  };
}

/**
 * Write STANDARDS.md file for a project.
 */
export function writeStandardsFile(
  workspaceRoot: string,
  projectId: string,
  standards: ProjectStandards,
): void {
  const dir = ensureProjectDir(workspaceRoot, projectId);
  const lines: string[] = [];

  lines.push("# Coding Standards", "");
  lines.push(`*Updated: ${standards.updatedAt}*`, "");

  for (const layer of standards.layers) {
    lines.push(`## L${layer.level}: ${layer.name}`, "");
    lines.push(`*${layer.description}*`, "");

    if (layer.rules.length === 0) {
      lines.push("*No rules defined at this layer.*", "");
      continue;
    }

    lines.push("| ID | Rule | Check | Severity |");
    lines.push("|----|------|-------|----------|");
    for (const rule of layer.rules) {
      const severityIcon = rule.severity === "error" ? "🔴" : rule.severity === "warn" ? "🟡" : "🔵";
      const enabledMark = rule.enabled ? "" : " *(disabled)*";
      lines.push(`| ${rule.id} | ${rule.name}${enabledMark} | ${rule.check} | ${severityIcon} ${rule.severity} |`);
    }
    lines.push("");
  }

  // Summary
  lines.push("## Summary", "");
  const errorCount = standards.effectiveRules.filter((r) => r.severity === "error").length;
  const warnCount = standards.effectiveRules.filter((r) => r.severity === "warn").length;
  const infoCount = standards.effectiveRules.filter((r) => r.severity === "info").length;
  lines.push(`**${standards.effectiveRules.length} active rules:** ${errorCount} errors, ${warnCount} warnings, ${infoCount} info`, "");

  // JSON trailer for machine reading
  lines.push("---", "");
  lines.push("```json");
  lines.push(JSON.stringify({
    effectiveRules: standards.effectiveRules.map((r) => ({
      id: r.id, level: r.level, name: r.name, check: r.check, severity: r.severity, enabled: r.enabled,
    })),
  }, null, 2));
  lines.push("```");

  const filePath = join(dir, "STANDARDS.md");
  atomicWriteFile(filePath, lines.join("\n"));
  log.debug(`Wrote STANDARDS.md for ${projectId}`);
}

/**
 * Read standards from a STANDARDS.md file.
 */
export function readStandardsFile(
  workspaceRoot: string,
  projectId: string,
): { effectiveRules: Array<{ id: string; level: number; name: string; check: string; severity: string; enabled: boolean }> } | null {
  const dir = getProjectDir(workspaceRoot, projectId);
  const filePath = join(dir, "STANDARDS.md");

  if (!existsSync(filePath)) return null;

  try {
    const content = readFileSync(filePath, "utf-8");
    const jsonMatch = content.match(/```json\n([\s\S]+?)\n```/);
    if (!jsonMatch?.[1]) return null;
    return JSON.parse(jsonMatch[1]);
  } catch {
    return null;
  }
}

/**
 * Create a user preference rule from a code review correction.
 * This is how L3 standards get populated over time.
 */
export function createUserPreferenceRule(
  correction: string,
  opts?: { severity?: "error" | "warn" | "info"; check?: CodingRule["check"] },
): CodingRule {
  // Generate an incrementing ID
  const id = `L3-USR-${Date.now().toString(36).toUpperCase()}`;

  return {
    id,
    level: 3,
    name: correction.slice(0, 80),
    description: correction,
    check: opts?.check ?? "review",
    severity: opts?.severity ?? "warn",
    enabled: true,
  };
}
