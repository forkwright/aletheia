// Bootstrap token audit — measures per-section token usage for a nous
import { loadConfig, resolveNous, resolveWorkspace } from "../taxis/loader.js";
import { assembleBootstrap } from "./bootstrap.js";
import { estimateTokens, estimateToolDefTokens } from "../hermeneus/token-counter.js";
import { ToolRegistry } from "../organon/registry.js";

export async function auditTokens(agentId: string): Promise<void> {
  const config = loadConfig();
  const nous = resolveNous(config, agentId);
  if (!nous) {
    console.error(`Agent "${agentId}" not found in config.`);
    process.exit(1);
  }

  const workspace = resolveWorkspace(config, nous);
  const bootstrap = assembleBootstrap(workspace, {
    maxTokens: config.agents.defaults.bootstrapMaxTokens,
  });

  const contextWindow = config.agents.defaults.contextTokens ?? 200000;
  const maxOutput = config.agents.defaults.maxOutputTokens ?? 16384;

  // Build tool defs for token estimation (empty registry — no dispatch context available)
  const tools = new ToolRegistry();
  const toolDefs = tools.getDefinitions({});
  const toolDefTokens = estimateToolDefTokens(toolDefs);

  // Parse bootstrap blocks for per-file breakdown
  const allBlocks = [...bootstrap.staticBlocks, ...bootstrap.dynamicBlocks];
  const blockDetails: Array<{ label: string; tokens: number; cache: string }> = [];

  for (const block of allBlocks) {
    const text = block.text;
    const tokens = estimateTokens(text);
    const firstLine = text.split("\n")[0]?.trim() ?? "(empty)";
    const label = firstLine.startsWith("# ") ? firstLine.slice(2) : firstLine.slice(0, 60);
    const cache = block.cache_control ? "cached" : "dynamic";
    blockDetails.push({ label, tokens, cache });
  }

  const totalBootstrap = bootstrap.totalTokens;
  const historyBudget = Math.max(0, contextWindow - totalBootstrap - toolDefTokens - maxOutput);
  const usedPct = ((totalBootstrap + toolDefTokens) / contextWindow * 100).toFixed(1);

  console.log(`\n  Bootstrap Token Audit — ${nous.name} (${agentId})\n`);
  console.log(`  ${"Section".padEnd(40)} ${"Tokens".padStart(8)}  ${"Cache".padStart(8)}`);
  console.log(`  ${"─".repeat(40)} ${"─".repeat(8)}  ${"─".repeat(8)}`);

  for (const d of blockDetails) {
    console.log(`  ${d.label.padEnd(40)} ${String(d.tokens).padStart(8)}  ${d.cache.padStart(8)}`);
  }

  console.log(`  ${"─".repeat(40)} ${"─".repeat(8)}  ${"─".repeat(8)}`);
  console.log(`  ${"Bootstrap subtotal".padEnd(40)} ${String(totalBootstrap).padStart(8)}`);
  console.log(`  ${"Tool definitions (" + toolDefs.length + " tools)".padEnd(40)} ${String(toolDefTokens).padStart(8)}`);
  console.log(`  ${"Max output".padEnd(40)} ${String(maxOutput).padStart(8)}`);
  console.log(`  ${"─".repeat(40)} ${"─".repeat(8)}`);
  console.log(`  ${"Available for history".padEnd(40)} ${String(historyBudget).padStart(8)}`);
  console.log(`  ${"Context window".padEnd(40)} ${String(contextWindow).padStart(8)}`);
  console.log(`\n  Overhead: ${usedPct}% of context window used by bootstrap + tools`);

  if (bootstrap.droppedFiles.length > 0) {
    console.log(`\n  Dropped files (exceeded budget): ${bootstrap.droppedFiles.join(", ")}`);
  }

  console.log();
}
