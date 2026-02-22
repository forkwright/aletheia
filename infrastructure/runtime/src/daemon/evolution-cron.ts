// Evolutionary config search — nightly mutation, benchmark evaluation, variant promotion
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { createLogger } from "../koina/logger.js";
import { PipelineConfigSchema, type PipelineConfig, savePipelineConfig } from "../nous/pipeline-config.js";
import type { ProviderRouter } from "../hermeneus/router.js";
import type { TextBlock } from "../hermeneus/anthropic.js";
import type { SessionStore } from "../mneme/store.js";
import type { AletheiaConfig } from "../taxis/schema.js";

const log = createLogger("daemon:evolution");

const MAX_VARIANTS = 5;
const AUTO_ADOPT_MS = 86_400_000;
const MIN_IMPROVEMENT_PCT = 10;

export interface ConfigVariant {
  id: string;
  config: PipelineConfig;
  score: number;
  parentId: string | null;
  generation: number;
  createdAt: string;
}

export interface EvolutionArchive {
  variants: ConfigVariant[];
  currentDefault: string;
  lastRunAt: string;
  pendingPromotion?: {
    variantId: string;
    score: number;
    currentScore: number;
    improvementPct: number;
    notifiedAt: string;
    autoAdoptAt: string;
  };
}

interface BenchmarkTask {
  sessionId: string;
  userMessage: string;
  turnSeq: number;
}

function getArchiveDir(workspace: string): string {
  return join(workspace, "..", "..", "shared", "evolution");
}

function getArchivePath(workspace: string, nousId: string): string {
  const dir = join(getArchiveDir(workspace), nousId);
  mkdirSync(dir, { recursive: true });
  return join(dir, "archive.json");
}

export function loadArchive(workspace: string, nousId: string): EvolutionArchive {
  const path = getArchivePath(workspace, nousId);
  if (!existsSync(path)) {
    return {
      variants: [{
        id: `v0-${Date.now().toString(36)}`,
        config: PipelineConfigSchema.parse({}),
        score: 0.5,
        parentId: null,
        generation: 0,
        createdAt: new Date().toISOString(),
      }],
      currentDefault: "",
      lastRunAt: "",
    };
  }
  try {
    return JSON.parse(readFileSync(path, "utf-8")) as EvolutionArchive;
  } catch {
    log.warn(`Invalid archive for ${nousId}, resetting`);
    return { variants: [], currentDefault: "", lastRunAt: "" };
  }
}

function saveArchive(workspace: string, nousId: string, archive: EvolutionArchive): void {
  writeFileSync(getArchivePath(workspace, nousId), JSON.stringify(archive, null, 2) + "\n");
}

function selectTopVariants(archive: EvolutionArchive, n: number): ConfigVariant[] {
  return [...archive.variants].sort((a, b) => b.score - a.score).slice(0, n);
}

function harvestBenchmarkTasks(
  store: SessionStore,
  nousId: string,
  maxTasks: number,
): BenchmarkTask[] {
  const signals = store.getSignalHistory(nousId, maxTasks * 3);
  const approvals = signals.filter((s) => s.signal === "approval");
  const tasks: BenchmarkTask[] = [];
  const seen = new Set<string>();

  for (const approval of approvals) {
    if (tasks.length >= maxTasks) break;
    const key = `${approval.sessionId}:${approval.turnSeq}`;
    if (seen.has(key)) continue;
    seen.add(key);

    const history = store.getHistory(approval.sessionId, { excludeDistilled: true });
    const userMsg = history.find(
      (m) => m.role === "user" && m.seq < approval.turnSeq,
    );
    if (userMsg) {
      tasks.push({
        sessionId: approval.sessionId,
        userMessage: userMsg.content.slice(0, 500),
        turnSeq: approval.turnSeq,
      });
    }
  }

  return tasks;
}

async function evaluateVariant(
  router: ProviderRouter,
  tasks: BenchmarkTask[],
  _variant: ConfigVariant,
  model: string,
): Promise<number> {
  if (tasks.length === 0) return 0.5;

  let totalScore = 0;
  let evaluated = 0;

  for (const task of tasks) {
    try {
      const result = await router.complete({
        model,
        system: "",
        messages: [
          {
            role: "user",
            content:
              `You are evaluating an AI agent's capability. Given this user request:\n\n` +
              `"${task.userMessage}"\n\n` +
              `Rate from 0.0 to 1.0 how well a general-purpose AI assistant would handle this. ` +
              `Consider complexity, domain knowledge required, and potential for error. ` +
              `Respond with ONLY a decimal number.`,
          },
        ],
        maxTokens: 32,
      });

      const textBlock = result.content.find((b): b is TextBlock => b.type === "text");
      const scoreText = (textBlock?.text ?? "").trim();
      const score = parseFloat(scoreText);
      if (!isNaN(score) && score >= 0 && score <= 1) {
        totalScore += score;
        evaluated++;
      }
    } catch (err) {
      log.debug(`Benchmark eval failed for task ${task.sessionId}:${task.turnSeq}: ${err instanceof Error ? err.message : err}`);
    }
  }

  return evaluated > 0 ? totalScore / evaluated : 0.5;
}

async function mutateVariant(
  router: ProviderRouter,
  variant: ConfigVariant,
  model: string,
): Promise<PipelineConfig | null> {
  try {
    const result = await router.complete({
      model,
      system: "",
      messages: [
        {
          role: "user",
          content:
            `You are optimizing an AI agent's pipeline configuration. ` +
            `Current config (score: ${variant.score.toFixed(2)}):\n\n` +
            `${JSON.stringify(variant.config, null, 2)}\n\n` +
            `Propose ONE small mutation to improve performance. Change only 1-2 parameters. ` +
            `Valid ranges: recall.limit (1-30), recall.minScore (0-1), recall.maxTokens (100-5000), ` +
            `tools.expiryTurns (1-50), notes.tokenCap (100-10000).\n\n` +
            `Respond with ONLY the complete JSON config object, no explanation.`,
        },
      ],
      maxTokens: 512,
    });

    const textBlock = result.content.find((b): b is TextBlock => b.type === "text");
    const text = (textBlock?.text ?? "").trim();
    const jsonMatch = text.match(/\{[\s\S]*\}/);
    if (!jsonMatch) return null;

    const parsed = JSON.parse(jsonMatch[0]) as unknown;
    const validated = PipelineConfigSchema.safeParse(parsed);
    if (!validated.success) {
      log.debug(`Mutation failed validation: ${validated.error.message}`);
      return null;
    }
    return validated.data;
  } catch (err) {
    log.debug(`Mutation generation failed: ${err instanceof Error ? err.message : err}`);
    return null;
  }
}

export async function runEvolutionCycle(
  store: SessionStore,
  router: ProviderRouter,
  config: AletheiaConfig,
  opts?: {
    model?: string;
    tasksPerAgent?: number;
    sendNotification?: (nousId: string, message: string) => Promise<void>;
  },
): Promise<{
  agentsProcessed: number;
  variantsCreated: number;
  promotions: number;
  errors: string[];
}> {
  const model = opts?.model ?? "claude-haiku-4-5-20251001";
  const tasksPerAgent = opts?.tasksPerAgent ?? 10;
  const result = { agentsProcessed: 0, variantsCreated: 0, promotions: 0, errors: [] as string[] };

  for (const agent of config.agents.list) {
    try {
      const archive = loadArchive(agent.workspace, agent.id);

      // Check pending auto-adopt
      if (archive.pendingPromotion) {
        const adoptTime = new Date(archive.pendingPromotion.autoAdoptAt).getTime();
        if (Date.now() >= adoptTime && archive.pendingPromotion.improvementPct >= MIN_IMPROVEMENT_PCT) {
          const variant = archive.variants.find((v) => v.id === archive.pendingPromotion!.variantId);
          if (variant) {
            savePipelineConfig(agent.workspace, variant.config);
            archive.currentDefault = variant.id;
            log.info(`Auto-adopted variant ${variant.id} for ${agent.id} (${archive.pendingPromotion.improvementPct.toFixed(1)}% improvement)`);
            result.promotions++;
          }
          delete archive.pendingPromotion;
        }
      }

      // Select top-2 and mutate
      const top = selectTopVariants(archive, 2);
      const tasks = harvestBenchmarkTasks(store, agent.id, tasksPerAgent);

      for (const parent of top) {
        const mutatedConfig = await mutateVariant(router, parent, model);
        if (!mutatedConfig) continue;

        const newVariant: ConfigVariant = {
          id: `v${parent.generation + 1}-${Date.now().toString(36)}`,
          config: mutatedConfig,
          score: 0,
          parentId: parent.id,
          generation: parent.generation + 1,
          createdAt: new Date().toISOString(),
        };

        // Evaluate
        newVariant.score = await evaluateVariant(router, tasks, newVariant, model);

        archive.variants.push(newVariant);
        result.variantsCreated++;

        // Cap archive size
        if (archive.variants.length > MAX_VARIANTS) {
          archive.variants.sort((a, b) => b.score - a.score);
          archive.variants = archive.variants.slice(0, MAX_VARIANTS);
        }
      }

      // Check if best variant beats current default
      const currentDefault = archive.variants.find((v) => v.id === archive.currentDefault);
      const best = selectTopVariants(archive, 1)[0];
      if (best && currentDefault && best.id !== currentDefault.id) {
        const improvement = ((best.score - currentDefault.score) / Math.max(currentDefault.score, 0.01)) * 100;
        if (improvement >= MIN_IMPROVEMENT_PCT && !archive.pendingPromotion) {
          archive.pendingPromotion = {
            variantId: best.id,
            score: best.score,
            currentScore: currentDefault.score,
            improvementPct: improvement,
            notifiedAt: new Date().toISOString(),
            autoAdoptAt: new Date(Date.now() + AUTO_ADOPT_MS).toISOString(),
          };

          if (opts?.sendNotification) {
            await opts.sendNotification(
              agent.id,
              `[Evolution] ${agent.name ?? agent.id} variant ${best.id} outperformed current ` +
              `(${best.score.toFixed(2)} vs ${currentDefault.score.toFixed(2)}, +${improvement.toFixed(1)}%). ` +
              `Auto-adopts in 24h.`,
            );
          }
          log.info(`Pending promotion for ${agent.id}: ${best.id} (${improvement.toFixed(1)}% improvement)`);
        }
      }

      archive.lastRunAt = new Date().toISOString();
      saveArchive(agent.workspace, agent.id, archive);
      result.agentsProcessed++;
    } catch (err) {
      const msg = `${agent.id}: ${err instanceof Error ? err.message : err}`;
      result.errors.push(msg);
      log.warn(`Evolution failed for ${agent.id}: ${msg}`);
    }
  }

  return result;
}
