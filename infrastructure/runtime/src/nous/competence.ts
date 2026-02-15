// Competence model — per-agent per-domain confidence tracking
import { existsSync, readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";

const log = createLogger("nous.competence");

export interface DomainScore {
  domain: string;
  score: number;       // 0.0-1.0, starts at 0.5
  corrections: number; // operator corrections (decreases score)
  successes: number;   // verified completions (increases score)
  disagreements: number; // cross-agent disagreements
  lastUpdated: string;
}

export interface AgentCompetence {
  nousId: string;
  domains: Record<string, DomainScore>;
  overallScore: number;
}

const CORRECTION_PENALTY = 0.05;
const SUCCESS_BONUS = 0.02;
const DISAGREEMENT_PENALTY = 0.01;

export class CompetenceModel {
  private data: Record<string, AgentCompetence> = {};
  private filePath: string;

  constructor(sharedRoot: string) {
    const dir = join(sharedRoot, "shared", "competence");
    mkdirSync(dir, { recursive: true });
    this.filePath = join(dir, "model.json");
    this.load();
  }

  private load(): void {
    if (existsSync(this.filePath)) {
      try {
        this.data = JSON.parse(readFileSync(this.filePath, "utf-8"));
      } catch {
        this.data = {};
      }
    }
  }

  private save(): void {
    writeFileSync(this.filePath, JSON.stringify(this.data, null, 2));
  }

  private ensureAgent(nousId: string): AgentCompetence {
    if (!this.data[nousId]) {
      this.data[nousId] = { nousId, domains: {}, overallScore: 0.5 };
    }
    return this.data[nousId]!;
  }

  private ensureDomain(nousId: string, domain: string): DomainScore {
    const agent = this.ensureAgent(nousId);
    if (!agent.domains[domain]) {
      agent.domains[domain] = {
        domain,
        score: 0.5,
        corrections: 0,
        successes: 0,
        disagreements: 0,
        lastUpdated: new Date().toISOString(),
      };
    }
    return agent.domains[domain]!;
  }

  recordCorrection(nousId: string, domain: string): void {
    const d = this.ensureDomain(nousId, domain);
    d.corrections++;
    d.score = Math.max(0.1, d.score - CORRECTION_PENALTY);
    d.lastUpdated = new Date().toISOString();
    this.recalcOverall(nousId);
    this.save();
    log.info(`Correction: ${nousId}/${domain} → ${d.score.toFixed(2)}`);
  }

  recordSuccess(nousId: string, domain: string): void {
    const d = this.ensureDomain(nousId, domain);
    d.successes++;
    d.score = Math.min(0.95, d.score + SUCCESS_BONUS);
    d.lastUpdated = new Date().toISOString();
    this.recalcOverall(nousId);
    this.save();
  }

  recordDisagreement(nousId: string, domain: string): void {
    const d = this.ensureDomain(nousId, domain);
    d.disagreements++;
    d.score = Math.max(0.1, d.score - DISAGREEMENT_PENALTY);
    d.lastUpdated = new Date().toISOString();
    this.recalcOverall(nousId);
    this.save();
  }

  getScore(nousId: string, domain: string): number {
    return this.data[nousId]?.domains[domain]?.score ?? 0.5;
  }

  getAgentCompetence(nousId: string): AgentCompetence | null {
    return this.data[nousId] ?? null;
  }

  // Find the best agent for a domain
  bestAgentForDomain(domain: string, exclude?: string[]): { nousId: string; score: number } | null {
    let best: { nousId: string; score: number } | null = null;

    for (const [nousId, agent] of Object.entries(this.data)) {
      if (exclude?.includes(nousId)) continue;
      const d = agent.domains[domain];
      if (d && (!best || d.score > best.score)) {
        best = { nousId, score: d.score };
      }
    }

    return best;
  }

  private recalcOverall(nousId: string): void {
    const agent = this.data[nousId];
    if (!agent) return;
    const domains = Object.values(agent.domains);
    if (domains.length === 0) {
      agent.overallScore = 0.5;
      return;
    }
    agent.overallScore = domains.reduce((sum, d) => sum + d.score, 0) / domains.length;
  }

  toJSON(): Record<string, AgentCompetence> {
    return this.data;
  }
}
