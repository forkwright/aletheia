import { a as loadConfig, i as estimateToolDefTokens, n as ToolRegistry, o as resolveNous, r as estimateTokens, s as resolveWorkspace, t as assembleBootstrap } from "./entry.mjs";

//#region src/nous/audit.ts
function auditTokens(agentId) {
	const config = loadConfig();
	const nous = resolveNous(config, agentId);
	if (!nous) {
		console.error(`Agent "${agentId}" not found in config.`);
		process.exit(1);
	}
	const bootstrap = assembleBootstrap(resolveWorkspace(config, nous), { maxTokens: config.agents.defaults.bootstrapMaxTokens });
	const contextWindow = config.agents.defaults.contextTokens;
	const maxOutput = config.agents.defaults.maxOutputTokens;
	const toolDefs = new ToolRegistry().getDefinitions({});
	const toolDefTokens = estimateToolDefTokens(toolDefs);
	const allBlocks = [...bootstrap.staticBlocks, ...bootstrap.dynamicBlocks];
	const blockDetails = [];
	for (const block of allBlocks) {
		const text = block.text;
		const tokens = estimateTokens(text);
		const firstLine = text.split("\n")[0]?.trim() ?? "(empty)";
		const label = firstLine.startsWith("# ") ? firstLine.slice(2) : firstLine.slice(0, 60);
		const cache = block.cache_control ? "cached" : "dynamic";
		blockDetails.push({
			label,
			tokens,
			cache
		});
	}
	const totalBootstrap = bootstrap.totalTokens;
	const historyBudget = Math.max(0, contextWindow - totalBootstrap - toolDefTokens - maxOutput);
	const usedPct = ((totalBootstrap + toolDefTokens) / contextWindow * 100).toFixed(1);
	console.log(`\n  Bootstrap Token Audit — ${nous.name} (${agentId})\n`);
	console.log(`  ${"Section".padEnd(40)} ${"Tokens".padStart(8)}  ${"Cache".padStart(8)}`);
	console.log(`  ${"─".repeat(40)} ${"─".repeat(8)}  ${"─".repeat(8)}`);
	for (const d of blockDetails) console.log(`  ${d.label.padEnd(40)} ${String(d.tokens).padStart(8)}  ${d.cache.padStart(8)}`);
	console.log(`  ${"─".repeat(40)} ${"─".repeat(8)}  ${"─".repeat(8)}`);
	console.log(`  ${"Bootstrap subtotal".padEnd(40)} ${String(totalBootstrap).padStart(8)}`);
	console.log(`  ${"Tool definitions (" + toolDefs.length + " tools)".padEnd(40)} ${String(toolDefTokens).padStart(8)}`);
	console.log(`  ${"Max output".padEnd(40)} ${String(maxOutput).padStart(8)}`);
	console.log(`  ${"─".repeat(40)} ${"─".repeat(8)}`);
	console.log(`  ${"Available for history".padEnd(40)} ${String(historyBudget).padStart(8)}`);
	console.log(`  ${"Context window".padEnd(40)} ${String(contextWindow).padStart(8)}`);
	console.log(`\n  Overhead: ${usedPct}% of context window used by bootstrap + tools`);
	if (bootstrap.droppedFiles.length > 0) console.log(`\n  Dropped files (exceeded budget): ${bootstrap.droppedFiles.join(", ")}`);
	console.log();
}

//#endregion
export { auditTokens };
//# sourceMappingURL=audit-BIQG4bEY.mjs.map