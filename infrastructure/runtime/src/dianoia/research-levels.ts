// ResearchLevels — right-sized research per phase (ENG-11)
//
// Not every phase needs 30 minutes of research. A CRUD endpoint phase
// needs L0 (skip). A new auth system needs L2 (standard). A novel
// architecture needs L3 (deep dive).
//
// Research level selection is based on:
// 1. Phase requirements complexity
// 2. Novelty (does the codebase already have similar code?)
// 3. Risk (what breaks if we get this wrong?)
// 4. User override (explicit level in planning config)

import { createLogger } from "../koina/logger.js";
import type { PlanningPhase, PlanningRequirement } from "./types.js";

const log = createLogger("dianoia:research-levels");

export type ResearchLevel = 0 | 1 | 2 | 3;

export interface ResearchLevelConfig {
  /** Research level (0-3) */
  level: ResearchLevel;
  /** Human-readable name */
  name: string;
  /** Description */
  description: string;
  /** Expected duration range */
  durationRange: string;
  /** Dimensions to research */
  dimensions: string[];
  /** Number of researchers to dispatch */
  researcherCount: number;
  /** Context budget per researcher */
  contextBudgetPerResearcher: number;
  /** Whether synthesis is needed */
  needsSynthesis: boolean;
}

/** Research level definitions */
export const RESEARCH_LEVELS: Record<ResearchLevel, ResearchLevelConfig> = {
  0: {
    level: 0,
    name: "Skip",
    description: "Well-understood domain, existing patterns. No research needed.",
    durationRange: "0 min",
    dimensions: [],
    researcherCount: 0,
    contextBudgetPerResearcher: 0,
    needsSynthesis: false,
  },
  1: {
    level: 1,
    name: "Quick",
    description: "Mostly understood with a few unknowns. Quick validation.",
    durationRange: "2-5 min",
    dimensions: ["pitfalls"],
    researcherCount: 1,
    contextBudgetPerResearcher: 4000,
    needsSynthesis: false,
  },
  2: {
    level: 2,
    name: "Standard",
    description: "New domain or significant complexity. Full 4-dimension research.",
    durationRange: "15-30 min",
    dimensions: ["stack", "features", "architecture", "pitfalls"],
    researcherCount: 4,
    contextBudgetPerResearcher: 6000,
    needsSynthesis: true,
  },
  3: {
    level: 3,
    name: "Deep Dive",
    description: "Novel architecture, high risk, or unfamiliar domain. Extended research with validation.",
    durationRange: "1+ hour",
    dimensions: ["stack", "features", "architecture", "pitfalls"],
    researcherCount: 4,
    contextBudgetPerResearcher: 10000,
    needsSynthesis: true,
  },
};

/** Signals that suggest higher research levels */
interface ComplexitySignals {
  /** Number of requirements in the phase */
  requirementCount: number;
  /** Whether requirements mention unfamiliar technologies */
  hasNovelTechnology: boolean;
  /** Whether requirements mention security/auth */
  hasSecurityConcerns: boolean;
  /** Whether requirements mention data migration */
  hasDataMigration: boolean;
  /** Whether requirements mention external integrations */
  hasExternalIntegrations: boolean;
  /** Whether the phase goal mentions architectural decisions */
  hasArchitecturalDecisions: boolean;
  /** Whether similar code exists in the codebase */
  hasExistingPatterns: boolean;
  /** Explicit user override (null = auto-detect) */
  userOverride: ResearchLevel | null;
}

/** Keywords that signal novel/unfamiliar technology */
const NOVEL_TECH_KEYWORDS = [
  "new framework", "migrate to", "replace", "novel", "unfamiliar",
  "first time", "proof of concept", "prototype", "experimental",
  "custom protocol", "from scratch",
];

/** Keywords that signal security concerns */
const SECURITY_KEYWORDS = [
  "auth", "authentication", "authorization", "oauth", "oidc", "jwt",
  "encryption", "certificate", "tls", "ssl", "permission", "rbac",
  "credential", "secret", "token", "password", "sanitize", "xss", "csrf",
];

/** Keywords that signal data migration */
const MIGRATION_KEYWORDS = [
  "migration", "migrate", "schema change", "data conversion",
  "backward compat", "breaking change", "database upgrade",
];

/** Keywords that signal external integrations */
const INTEGRATION_KEYWORDS = [
  "api integration", "third-party", "external service", "webhook",
  "sdk", "provider", "vendor", "saas", "cloud service",
];

/** Keywords that signal architectural decisions */
const ARCHITECTURE_KEYWORDS = [
  "architecture", "design pattern", "system design", "scalab",
  "distributed", "microservice", "monolith", "event-driven",
  "message queue", "caching strategy", "load balanc",
];

/**
 * Extract complexity signals from a phase and its requirements.
 */
export function extractComplexitySignals(
  phase: PlanningPhase,
  requirements: PlanningRequirement[],
  opts?: { userOverride?: ResearchLevel | null; existingPatterns?: boolean },
): ComplexitySignals {
  const text = [
    phase.goal,
    phase.name,
    ...phase.successCriteria,
    ...requirements.map((r) => r.description),
  ].join(" ").toLowerCase();

  return {
    requirementCount: requirements.length,
    hasNovelTechnology: NOVEL_TECH_KEYWORDS.some((kw) => text.includes(kw)),
    hasSecurityConcerns: SECURITY_KEYWORDS.some((kw) => text.includes(kw)),
    hasDataMigration: MIGRATION_KEYWORDS.some((kw) => text.includes(kw)),
    hasExternalIntegrations: INTEGRATION_KEYWORDS.some((kw) => text.includes(kw)),
    hasArchitecturalDecisions: ARCHITECTURE_KEYWORDS.some((kw) => text.includes(kw)),
    hasExistingPatterns: opts?.existingPatterns ?? false,
    userOverride: opts?.userOverride ?? null,
  };
}

/**
 * Select the appropriate research level for a phase.
 */
export function selectResearchLevel(signals: ComplexitySignals): ResearchLevel {
  // User override takes priority
  if (signals.userOverride !== null) {
    log.info(`Using user-overridden research level: L${signals.userOverride}`);
    return signals.userOverride;
  }

  let score = 0;

  // Requirement count
  if (signals.requirementCount <= 2) score += 0;
  else if (signals.requirementCount <= 5) score += 1;
  else if (signals.requirementCount <= 10) score += 2;
  else score += 3;

  // Novelty and risk signals
  if (signals.hasNovelTechnology) score += 3;
  if (signals.hasSecurityConcerns) score += 2;
  if (signals.hasDataMigration) score += 2;
  if (signals.hasExternalIntegrations) score += 2;
  if (signals.hasArchitecturalDecisions) score += 2;

  // Existing patterns reduce need
  if (signals.hasExistingPatterns) score -= 2;

  // Map score to level
  if (score <= 0) return 0;      // Skip
  if (score <= 2) return 1;      // Quick
  if (score <= 6) return 2;      // Standard
  return 3;                       // Deep Dive
}

/**
 * Get the research config for a level.
 */
export function getResearchConfig(level: ResearchLevel): ResearchLevelConfig {
  return RESEARCH_LEVELS[level];
}

/**
 * Determine research level for a phase automatically.
 * Combines signal extraction and level selection in one call.
 */
export function determineResearchLevel(
  phase: PlanningPhase,
  requirements: PlanningRequirement[],
  opts?: { userOverride?: ResearchLevel | null; existingPatterns?: boolean },
): { level: ResearchLevel; config: ResearchLevelConfig; signals: ComplexitySignals } {
  const signals = extractComplexitySignals(phase, requirements, opts);
  const level = selectResearchLevel(signals);
  const config = getResearchConfig(level);

  log.info(
    `Research level for "${phase.name}": L${level} (${config.name}) — ` +
    `${signals.requirementCount} reqs, ` +
    `novel=${signals.hasNovelTechnology}, ` +
    `security=${signals.hasSecurityConcerns}, ` +
    `migration=${signals.hasDataMigration}`,
  );

  return { level, config, signals };
}
