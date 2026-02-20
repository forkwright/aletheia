// Ephemeral agent spawning — Syn creates temporary specialists with bounded lifecycles
import { mkdirSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { join } from "node:path";
import { randomBytes } from "node:crypto";
import { createLogger } from "../koina/logger.js";
import { PipelineError } from "../koina/errors.js";

const log = createLogger("nous.ephemeral");

const MAX_CONCURRENT = 3;

export interface EphemeralSpec {
  name: string;
  soul: string;       // SOUL.md content — the specialist's identity and capabilities
  maxTurns: number;    // Hard limit on turns before teardown
  maxDurationMs: number; // Hard timeout
  tools?: string[];    // Optional tool allowlist (empty = all tools)
}

export interface EphemeralAgent {
  id: string;
  spec: EphemeralSpec;
  workspace: string;
  turnCount: number;
  createdAt: number;
  expiresAt: number;
  output: string[];
}

const activeEphemerals = new Map<string, EphemeralAgent>();

export function spawnEphemeral(spec: EphemeralSpec, sharedRoot: string): EphemeralAgent {
  if (activeEphemerals.size >= MAX_CONCURRENT) {
    // Evict oldest expired
    const now = Date.now();
    for (const [id, agent] of activeEphemerals) {
      if (now > agent.expiresAt || agent.turnCount >= agent.spec.maxTurns) {
        teardownEphemeral(id);
        break;
      }
    }
    if (activeEphemerals.size >= MAX_CONCURRENT) {
      throw new PipelineError(`Maximum ${MAX_CONCURRENT} concurrent ephemeral agents`, {
        code: "EPHEMERAL_LIMIT", context: { active: activeEphemerals.size, max: MAX_CONCURRENT },
      });
    }
  }

  const id = `eph_${randomBytes(16).toString("hex")}`;
  const parentDir = join(sharedRoot, "ephemeral");
  mkdirSync(parentDir, { recursive: true });
  const workspace = join(parentDir, id);
  mkdirSync(workspace, { mode: 0o700 }); // codeql[js/insecure-temporary-file] - dedicated ephemeral workspace under sharedRoot, not tmpdir

  // Write SOUL.md for the specialist
  writeFileSync(join(workspace, "SOUL.md"), spec.soul);

  const agent: EphemeralAgent = {
    id,
    spec,
    workspace,
    turnCount: 0,
    createdAt: Date.now(),
    expiresAt: Date.now() + spec.maxDurationMs,
    output: [],
  };

  activeEphemerals.set(id, agent);
  log.info(`Spawned ephemeral agent ${id} (${spec.name}), max ${spec.maxTurns} turns, ${spec.maxDurationMs / 1000}s`);

  return agent;
}

export function recordEphemeralTurn(id: string, response: string): boolean {
  const agent = activeEphemerals.get(id);
  if (!agent) return false;

  agent.turnCount++;
  agent.output.push(response);

  if (agent.turnCount >= agent.spec.maxTurns || Date.now() > agent.expiresAt) {
    log.info(`Ephemeral ${id} reached limit (${agent.turnCount} turns)`);
    return false; // Signal caller to teardown
  }

  return true; // Can continue
}

export function teardownEphemeral(id: string): EphemeralAgent | null {
  const agent = activeEphemerals.get(id);
  if (!agent) return null;

  activeEphemerals.delete(id);

  // Clean up workspace
  try {
    if (existsSync(agent.workspace)) {
      rmSync(agent.workspace, { recursive: true, force: true });
    }
  } catch (err) {
    log.warn(`Failed to clean ephemeral workspace ${id}: ${err instanceof Error ? err.message : err}`);
  }

  log.info(`Torn down ephemeral ${id} after ${agent.turnCount} turns`);
  return agent;
}

export function getEphemeral(id: string): EphemeralAgent | undefined {
  return activeEphemerals.get(id);
}

export function listEphemerals(): EphemeralAgent[] {
  return Array.from(activeEphemerals.values());
}

export function harvestOutput(id: string): string {
  const agent = activeEphemerals.get(id);
  if (!agent) return "";
  return agent.output.join("\n\n---\n\n");
}
