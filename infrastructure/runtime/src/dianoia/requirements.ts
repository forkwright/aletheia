// RequirementsOrchestrator — manages requirements scoping loop: categories → decisions → coverage gate
import type Database from "better-sqlite3";
import { createLogger } from "../koina/logger.js";
import { PlanningError } from "../koina/errors.js";
import { PlanningStore } from "./store.js";
import { transition } from "./machine.js";
import { writeRequirementsFile } from "./project-files.js";

const log = createLogger("dianoia:requirements");

export interface FeatureProposal {
  name: string;
  description: string;
  isTableStakes: boolean;
  proposedTier: "v1" | "v2" | "out-of-scope";
  proposedRationale?: string;
}

export interface CategoryProposal {
  category: string;
  categoryName: string;
  tableStakes: FeatureProposal[];
  differentiators: FeatureProposal[];
}

export interface ScopingDecision {
  name: string;
  tier: "v1" | "v2" | "out-of-scope";
  rationale?: string;
}

export class RequirementsOrchestrator {
  private store: PlanningStore;

  constructor(db: Database.Database, private workspaceRoot?: string) {
    this.store = new PlanningStore(db);
  }

  getSynthesis(projectId: string): string | null {
    const rows = this.store.listResearch(projectId);
    const synthesis = rows.find((r) => r.dimension === "synthesis");
    return synthesis?.content ?? null;
  }

  formatCategoryPresentation(category: CategoryProposal): string {
    const lines: string[] = [
      `## ${category.categoryName} (${category.category})`,
      "",
      "**Table stakes** (users expect these):",
    ];

    for (const f of category.tableStakes) {
      lines.push(`- **${f.name}**: ${f.description} → proposed: **${f.proposedTier}**`);
    }

    if (category.differentiators.length > 0) {
      lines.push("");
      lines.push("**Differentiators** (set your product apart):");
      for (const f of category.differentiators) {
        lines.push(`- **${f.name}**: ${f.description} → proposed: **${f.proposedTier}**`);
      }
    }

    lines.push("");
    lines.push(
      "Confirm these proposals or adjust (e.g., 'move the second one to v2', 'make all v1'):",
    );

    return lines.join("\n");
  }

  persistCategory(
    projectId: string,
    category: CategoryProposal,
    decisions: ScopingDecision[],
  ): void {
    const existing = this.store
      .listRequirements(projectId)
      .filter((r) => r.category === category.category);

    let nextNum = 1;
    if (existing.length > 0) {
      const maxNum = existing.reduce((max, r) => {
        const match = /-(\d+)$/.exec(r.reqId);
        const num = match ? parseInt(match[1]!, 10) : 0;
        return Math.max(max, num);
      }, 0);
      nextNum = maxNum + 1;
    }

    const allFeatures = [...category.tableStakes, ...category.differentiators];
    const allExistingReqs = this.store.listRequirements(projectId);

    for (const decision of decisions) {
      const reqId = `${category.category}-${String(nextNum).padStart(2, "0")}`;

      // Check for duplicate reqId
      if (allExistingReqs.some((r) => r.reqId === reqId)) {
        throw new PlanningError(`Duplicate requirement ID: ${reqId}`, {
          code: "PLANNING_DUPLICATE_REQUIREMENT_ID",
          context: { reqId, projectId },
        });
      }

      const feature = allFeatures.find((f) => f.name === decision.name);
      
      // Table-stakes enforcement
      if (feature && feature.isTableStakes && decision.tier === "out-of-scope") {
        if (!decision.rationale || decision.rationale.trim() === "") {
          throw new PlanningError(`Table-stakes feature "${decision.name}" marked as out-of-scope without rationale`, {
            code: "PLANNING_TABLE_STAKES_OUT_OF_SCOPE",
            context: { featureName: decision.name, projectId },
          });
        }
      }

      let description = feature?.description ?? decision.name;

      if (!description.startsWith("User can") && !/can |is able to |allows |enables /i.test(description)) {
        description = `User can ${description.charAt(0).toLowerCase()}${description.slice(1)}`;
      }

      this.store.createRequirement({
        projectId,
        reqId,
        description,
        category: category.category,
        tier: decision.tier,
        rationale: decision.rationale ?? null,
      });

      nextNum++;
    }

    // Write REQUIREMENTS.md after each category persist
    if (this.workspaceRoot) {
      const allRequirements = this.store.listRequirements(projectId);
      writeRequirementsFile(this.workspaceRoot, projectId, allRequirements);
    }

    log.info(`Persisted ${decisions.length} requirements for category ${category.category}`);
  }

  updateRequirement(
    projectId: string,
    reqId: string,
    updates: { tier?: "v1" | "v2" | "out-of-scope"; rationale?: string | null },
  ): void {
    const all = this.store.listRequirements(projectId);
    const row = all.find((r) => r.reqId === reqId);
    if (!row) {
      throw new PlanningError(`Requirement not found: ${reqId}`, {
        code: "PLANNING_REQUIREMENT_NOT_FOUND",
        context: { reqId, projectId },
      });
    }
    this.store.updateRequirement(row.id, updates);
  }

  validateCoverage(projectId: string, presentedCategories: string[], minimumCategories = 1): boolean {
    const reqs = this.store.listRequirements(projectId);

    // Minimum category count gate
    if (presentedCategories.length < minimumCategories) {
      log.debug(`Coverage gate failed: only ${presentedCategories.length} categories, minimum ${minimumCategories} required`);
      return false;
    }

    // At least one v1 requirement
    const hasV1 = reqs.some((r) => r.tier === "v1");
    if (!hasV1) {
      log.debug(`Coverage gate failed: no v1 requirements found`);
      return false;
    }

    // Every presented category has at least one requirement
    for (const cat of presentedCategories) {
      const hasCoverage = reqs.some((r) => r.category === cat);
      if (!hasCoverage) {
        log.debug(`Coverage gate failed: category ${cat} has no requirements`);
        return false;
      }
    }

    log.debug(`Coverage gate passed: ${presentedCategories.length} categories, ${reqs.filter(r => r.tier === "v1").length} v1 requirements`);
    return true;
  }

  transitionToRoadmap(projectId: string): void {
    this.store.updateProjectState(projectId, transition("requirements", "REQUIREMENTS_COMPLETE"));
  }
}
