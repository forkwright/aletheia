// One-shot sufficiency threshold tuning script
// Queries the sidecar /graph_enhanced_search endpoint against corpus expected facts
// and measures precision/recall at threshold values from 0.1 to 0.9 in 0.05 steps.
//
// Prerequisites: ANTHROPIC_API_KEY set, sidecar running (SIDECAR_URL env var or default).
// Usage: npm run test:tune-sufficiency
//
// Output: threshold → precision, recall, F1 table + recommended values for pipeline.json

import { readdirSync, readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import type { AnnotatedConversation } from "./types.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const CONVERSATIONS_DIR = join(__dirname, "conversations");

const SIDECAR_URL = process.env["SIDECAR_URL"] ?? "http://localhost:8765";
const USER_ID = process.env["SIDECAR_USER_ID"] ?? "default";
const SIMILARITY_THRESHOLD = parseFloat(process.env["SIMILARITY_THRESHOLD"] ?? "0.3");

const THRESHOLD_MIN = 0.10;
const THRESHOLD_MAX = 0.90;
const THRESHOLD_STEP = 0.05;

// Minimum recall at optimal precision (conservative threshold selection criterion)
const MIN_RECALL_FOR_CONSERVATIVE = 0.70;

interface SearchResult {
  id: string;
  memory: string;
  score: number;
}

interface ThresholdMetrics {
  threshold: number;
  precision: number;
  recall: number;
  f1: number;
  found: number;
  surfaced: number;
  expected: number;
}

// --- Jaccard token similarity (same as run-corpus.ts) ---

function computeSimilarity(a: string, b: string): number {
  const tokenize = (s: string) =>
    new Set(
      s.toLowerCase().replace(/[^a-z0-9\s]/g, " ").split(/\s+/).filter((t) => t.length > 2),
    );
  const ta = tokenize(a);
  const tb = tokenize(b);
  const intersection = [...ta].filter((t) => tb.has(t)).length;
  const union = new Set([...ta, ...tb]).size;
  return union === 0 ? 0 : intersection / union;
}

function matchFacts(surfaced: string[], expected: string[]): { found: number } {
  let found = 0;
  const usedExpected = new Set<number>();
  for (const s of surfaced) {
    for (let i = 0; i < expected.length; i++) {
      if (usedExpected.has(i)) continue;
      if (computeSimilarity(s, expected[i]!) >= SIMILARITY_THRESHOLD) {
        found++;
        usedExpected.add(i);
        break;
      }
    }
  }
  return { found };
}

// --- Sidecar search ---

async function searchSidecar(
  query: string,
  agentId: string,
  limit: number = 20,
): Promise<SearchResult[]> {
  const resp = await fetch(`${SIDECAR_URL}/graph_enhanced_search`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ query, user_id: USER_ID, agent_id: agentId, limit }),
  });
  if (!resp.ok) {
    throw new Error(`Sidecar search failed: ${resp.status} ${await resp.text()}`);
  }
  const data = (await resp.json()) as { ok: boolean; results: SearchResult[] };
  return data.results ?? [];
}

// --- Main ---

async function main() {
  // Check sidecar reachability
  try {
    const health = await fetch(`${SIDECAR_URL}/health`);
    if (!health.ok) throw new Error(`status ${health.status}`);
    console.log(`Sidecar health: OK (${SIDECAR_URL})`);
  } catch (err) {
    console.error(`ERROR: Cannot reach sidecar at ${SIDECAR_URL}`);
    console.error(`  Start the sidecar first, or set SIDECAR_URL env var.`);
    console.error(`  Error: ${err instanceof Error ? err.message : err}`);
    process.exit(1);
  }

  const files = readdirSync(CONVERSATIONS_DIR).filter((f) => f.endsWith(".json"));
  console.log(`\nLoaded ${files.length} corpus conversations`);

  const conversations = files.map((f) =>
    JSON.parse(readFileSync(join(CONVERSATIONS_DIR, f), "utf-8")) as AnnotatedConversation,
  );

  // Build threshold range
  const thresholds: number[] = [];
  for (let t = THRESHOLD_MIN; t <= THRESHOLD_MAX + 0.001; t += THRESHOLD_STEP) {
    thresholds.push(Math.round(t * 100) / 100);
  }

  // For each conversation, search the sidecar using the first expected fact as the query
  // and collect all results with their scores. We'll filter by threshold offline.
  console.log(`\nQuerying sidecar for ${conversations.length} conversations...`);
  const allResults: Array<{ results: SearchResult[]; expected: string[] }> = [];

  for (const conv of conversations) {
    const expectedFacts = conv.expected.facts;
    if (expectedFacts.length === 0) continue;

    // Use each expected fact as a probe query (simulates agent recall during a session)
    for (const fact of expectedFacts.slice(0, 3)) {
      try {
        const results = await searchSidecar(fact, conv.agent);
        allResults.push({ results, expected: expectedFacts });
      } catch (err) {
        console.warn(`  SKIP ${conv.id}: search failed — ${err instanceof Error ? err.message : err}`);
      }
    }

    process.stdout.write(".");
  }
  console.log(` done (${allResults.length} queries)\n`);

  // Evaluate each threshold
  const metrics: ThresholdMetrics[] = [];

  for (const threshold of thresholds) {
    let totalFound = 0;
    let totalSurfaced = 0;
    let totalExpected = 0;

    for (const { results, expected } of allResults) {
      // Filter results by score >= threshold (this simulates sufficiency gate)
      const surfaced = results
        .filter((r) => r.score >= threshold)
        .map((r) => r.memory);

      const { found } = matchFacts(surfaced, expected);
      totalFound += found;
      totalSurfaced += surfaced.length;
      totalExpected += expected.length;
    }

    const precision = totalSurfaced === 0 ? 1.0 : totalFound / totalSurfaced;
    const recall = totalExpected === 0 ? 1.0 : totalFound / totalExpected;
    const f1 = precision + recall === 0 ? 0 : (2 * precision * recall) / (precision + recall);

    metrics.push({ threshold, precision, recall, f1, found: totalFound, surfaced: totalSurfaced, expected: totalExpected });
  }

  // Print results table
  console.log("--- Threshold tuning results ---\n");
  console.log(
    `${"Threshold".padEnd(12)} ${"Precision".padEnd(12)} ${"Recall".padEnd(12)} ${"F1".padEnd(12)} ${"Surfaced".padEnd(10)} Found/Expected`,
  );
  console.log("-".repeat(70));
  for (const m of metrics) {
    console.log(
      `${m.threshold.toFixed(2).padEnd(12)} ${m.precision.toFixed(3).padEnd(12)} ${m.recall.toFixed(3).padEnd(12)} ${m.f1.toFixed(3).padEnd(12)} ${String(m.surfaced).padEnd(10)} ${m.found}/${m.expected}`,
    );
  }

  // Find optimal (max F1) and conservative (max precision with recall > MIN_RECALL)
  const optimalMetric = metrics.reduce((best, m) => (m.f1 > best.f1 ? m : best), metrics[0]!);
  const conservativeCandidates = metrics.filter((m) => m.recall >= MIN_RECALL_FOR_CONSERVATIVE);
  const conservativeMetric = conservativeCandidates.length > 0
    ? conservativeCandidates.reduce((best, m) => (m.precision > best.precision ? m : best), conservativeCandidates[0]!)
    : optimalMetric;

  console.log(`\n--- Recommendations ---\n`);
  console.log(`Optimal threshold (max F1):          ${optimalMetric.threshold.toFixed(2)}`);
  console.log(`  precision=${optimalMetric.precision.toFixed(3)} recall=${optimalMetric.recall.toFixed(3)} f1=${optimalMetric.f1.toFixed(3)}`);
  console.log(`\nConservative threshold (max precision with recall >= ${MIN_RECALL_FOR_CONSERVATIVE}):`);
  console.log(`                                     ${conservativeMetric.threshold.toFixed(2)}`);
  console.log(`  precision=${conservativeMetric.precision.toFixed(3)} recall=${conservativeMetric.recall.toFixed(3)} f1=${conservativeMetric.f1.toFixed(3)}`);

  console.log(`\n--- Recommended pipeline.json ---\n`);
  console.log(`{`);
  console.log(`  "recall": {`);
  console.log(`    "sufficiencyThreshold": ${optimalMetric.threshold.toFixed(2)},`);
  console.log(`    "sufficiencyMinHits": 3`);
  console.log(`  }`);
  console.log(`}`);
  console.log(`\nFor higher precision at the cost of recall, use ${conservativeMetric.threshold.toFixed(2)} instead.`);
  console.log(`Review results and commit chosen threshold to each agent's pipeline.json.`);
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
