// Per-agent pipeline configuration — recall, tool expiry, note budget
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { z } from "zod";
import { createLogger } from "../koina/logger.js";

const log = createLogger("pipeline-config");

export const RecallConfigSchema = z.object({
  limit: z.number().int().min(1).max(30).default(8),
  maxTokens: z.number().int().min(100).max(5000).default(1500),
  minScore: z.number().min(0).max(1).default(0.75),
  sufficiencyThreshold: z.number().min(0).max(1).default(0.85),
  sufficiencyMinHits: z.number().int().min(1).max(20).default(3),
}).default({});

export const ToolsConfigSchema = z.object({
  expiryTurns: z.number().int().min(1).max(50).default(5),
}).default({});

export const NotesConfigSchema = z.object({
  tokenCap: z.number().int().min(100).max(10000).default(2000),
}).default({});

export const PipelineConfigSchema = z.object({
  recall: RecallConfigSchema,
  tools: ToolsConfigSchema,
  notes: NotesConfigSchema,
}).default({});

export type PipelineConfig = z.infer<typeof PipelineConfigSchema>;

const DEFAULTS: PipelineConfig = PipelineConfigSchema.parse({});

interface CacheEntry {
  config: PipelineConfig;
  mtimeMs: number;
}

const cache = new Map<string, CacheEntry>();

export function loadPipelineConfig(workspace: string): PipelineConfig {
  const filePath = join(workspace, "pipeline.json");

  if (!existsSync(filePath)) return DEFAULTS;

  try {
    const stat = statSync(filePath);
    const cached = cache.get(filePath);
    if (cached && cached.mtimeMs === stat.mtimeMs) return cached.config;

    const raw = readFileSync(filePath, "utf-8");
    const parsed = JSON.parse(raw) as unknown;
    const result = PipelineConfigSchema.parse(parsed);
    cache.set(filePath, { config: result, mtimeMs: stat.mtimeMs });
    return result;
  } catch (err) {
    log.warn(`Invalid pipeline.json in ${workspace}: ${err instanceof Error ? err.message : err}`);
    return DEFAULTS;
  }
}

export function savePipelineConfig(workspace: string, config: PipelineConfig): void {
  const filePath = join(workspace, "pipeline.json");
  writeFileSync(filePath, JSON.stringify(config, null, 2) + "\n");
  clearPipelineConfigCache(workspace);
}

export function clearPipelineConfigCache(workspace?: string): void {
  if (workspace) {
    cache.delete(join(workspace, "pipeline.json"));
  } else {
    cache.clear();
  }
}
