// Corpus benchmark runner — measures extraction precision/recall against annotated ground truth
import { readdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { extractFromMessages } from "../../src/distillation/extract.js";
import { ProviderRouter } from "../../src/hermeneus/router.js";
import { AnthropicProvider } from "../../src/hermeneus/anthropic.js";
import type { AnnotatedConversation, PerTypeMetrics, BaselineFile } from "./types.js";

const __dirname = dirname(fileURLToPath(import.meta.url));
const CONVERSATIONS_DIR = join(__dirname, "conversations");
const BASELINE_PATH = join(__dirname, "baseline.json");

const SIMILARITY_THRESHOLD = parseFloat(process.env["SIMILARITY_THRESHOLD"] ?? "0.3");
const REGRESSION_THRESHOLD = parseFloat(process.env["REGRESSION_THRESHOLD"] ?? "0.05");
const CORPUS_MODEL = process.env["CORPUS_MODEL"] ?? "claude-sonnet-4-20250514";
const SAVE_BASELINE = process.argv.includes("--save-baseline");

// --- Jaccard similarity ---

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

export function matchItems(extracted: string[], expected: string[], threshold: number): PerTypeMetrics {
  let matched = 0;
  const usedExpected = new Set<number>();

  for (const ext of extracted) {
    for (let i = 0; i < expected.length; i++) {
      if (usedExpected.has(i)) continue;
      if (computeSimilarity(ext, expected[i]!) >= threshold) {
        matched++;
        usedExpected.add(i);
        break;
      }
    }
  }

  const precision = extracted.length === 0 ? 1 : matched / extracted.length;
  const recall = expected.length === 0 ? 1 : matched / expected.length;
  const f1 = precision + recall === 0 ? 0 : (2 * precision * recall) / (precision + recall);
  return { precision, recall, f1, matched, extracted: extracted.length, expected: expected.length };
}

function aggregateMetrics(items: PerTypeMetrics[]): PerTypeMetrics {
  const totals = items.reduce(
    (acc, m) => ({
      matched: acc.matched + m.matched,
      extracted: acc.extracted + m.extracted,
      expected: acc.expected + m.expected,
    }),
    { matched: 0, extracted: 0, expected: 0 },
  );
  const precision = totals.extracted === 0 ? 1 : totals.matched / totals.extracted;
  const recall = totals.expected === 0 ? 1 : totals.matched / totals.expected;
  const f1 = precision + recall === 0 ? 0 : (2 * precision * recall) / (precision + recall);
  return { ...totals, precision, recall, f1 };
}

// --- Router setup ---

function buildRouter(): ProviderRouter {
  const apiKey = process.env["ANTHROPIC_API_KEY"];
  if (!apiKey) {
    console.error("ERROR: ANTHROPIC_API_KEY is not set. Export it before running the corpus benchmark.");
    process.exit(1);
  }
  const router = new ProviderRouter();
  router.registerProvider("anthropic", new AnthropicProvider({ apiKey }), []);
  return router;
}

// --- Main ---

async function main() {
  const router = buildRouter();

  const files = readdirSync(CONVERSATIONS_DIR).filter((f) => f.endsWith(".json"));
  console.log(`\nLoaded ${files.length} corpus conversations`);

  const allTypeMetrics: { facts: PerTypeMetrics[]; decisions: PerTypeMetrics[]; contradictions: PerTypeMetrics[]; entities: PerTypeMetrics[] } = { facts: [], decisions: [], contradictions: [], entities: [] };
  const perAgentMetrics: Record<string, PerTypeMetrics[]> = {};

  for (const file of files) {
    const conv = JSON.parse(readFileSync(join(CONVERSATIONS_DIR, file), "utf-8")) as AnnotatedConversation;
    let result;
    try {
      result = await extractFromMessages(router, conv.messages, CORPUS_MODEL);
    } catch (err) {
      console.warn(`  SKIP ${conv.id}: extraction failed — ${err instanceof Error ? err.message : err}`);
      continue;
    }

    const facts = matchItems(result.facts, conv.expected.facts, SIMILARITY_THRESHOLD);
    const decisions = matchItems(result.decisions, conv.expected.decisions, SIMILARITY_THRESHOLD);
    const contradictions = matchItems(result.contradictions, conv.expected.contradictions, SIMILARITY_THRESHOLD);
    const entities = matchItems(result.keyEntities, conv.expected.entities, SIMILARITY_THRESHOLD);

    allTypeMetrics.facts.push(facts);
    allTypeMetrics.decisions.push(decisions);
    allTypeMetrics.contradictions.push(contradictions);
    allTypeMetrics.entities.push(entities);

    const convAggregate = aggregateMetrics([facts, decisions, contradictions, entities]);
    perAgentMetrics[conv.agent] ??= [];
    perAgentMetrics[conv.agent]!.push(convAggregate);

    console.log(`  ${conv.id} (${conv.agent}): precision=${convAggregate.precision.toFixed(2)} recall=${convAggregate.recall.toFixed(2)} f1=${convAggregate.f1.toFixed(2)}`);
  }

  // Per-type aggregates
  const perType = {
    facts: aggregateMetrics(allTypeMetrics.facts),
    decisions: aggregateMetrics(allTypeMetrics.decisions),
    contradictions: aggregateMetrics(allTypeMetrics.contradictions),
    entities: aggregateMetrics(allTypeMetrics.entities),
  };
  const aggregate = aggregateMetrics(Object.values(perType));

  // Per-agent aggregates
  const perAgent: Record<string, PerTypeMetrics> = {};
  for (const [agent, metrics] of Object.entries(perAgentMetrics)) {
    perAgent[agent] = aggregateMetrics(metrics);
  }

  // Report
  console.log("\n--- Per-type metrics ---");
  console.table(
    Object.fromEntries(
      Object.entries(perType).map(([type, m]) => [
        type,
        { precision: m.precision.toFixed(3), recall: m.recall.toFixed(3), f1: m.f1.toFixed(3), matched: m.matched, extracted: m.extracted, expected: m.expected },
      ]),
    ),
  );

  console.log("\n--- Per-agent metrics ---");
  console.table(
    Object.fromEntries(
      Object.entries(perAgent).map(([agent, m]) => [
        agent,
        { precision: m.precision.toFixed(3), recall: m.recall.toFixed(3), f1: m.f1.toFixed(3) },
      ]),
    ),
  );

  console.log(`\nAggregate: precision=${aggregate.precision.toFixed(3)} recall=${aggregate.recall.toFixed(3)} f1=${aggregate.f1.toFixed(3)}`);

  if (SAVE_BASELINE) {
    const baseline: BaselineFile = {
      version: "1.0",
      generatedAt: new Date().toISOString(),
      corpusSize: files.length,
      aggregate,
      perType,
      perAgent,
    };
    writeFileSync(BASELINE_PATH, JSON.stringify(baseline, null, 2), "utf-8");
    console.log(`\nBaseline saved to ${BASELINE_PATH}`);
    process.exit(0);
  }

  // Regression check
  if (!existsSync(BASELINE_PATH)) {
    console.error("\nNo baseline found. Run with --save-baseline first.");
    process.exit(1);
  }
  const baseline = JSON.parse(readFileSync(BASELINE_PATH, "utf-8")) as BaselineFile;
  const pdelta = aggregate.precision - baseline.aggregate.precision;
  const rdelta = aggregate.recall - baseline.aggregate.recall;

  if (-pdelta > REGRESSION_THRESHOLD || -rdelta > REGRESSION_THRESHOLD) {
    console.error(`\nREGRESSION DETECTED`);
    console.error(`  precision: ${baseline.aggregate.precision.toFixed(3)} → ${aggregate.precision.toFixed(3)} (${pdelta >= 0 ? "+" : ""}${pdelta.toFixed(3)})`);
    console.error(`  recall:    ${baseline.aggregate.recall.toFixed(3)} → ${aggregate.recall.toFixed(3)} (${rdelta >= 0 ? "+" : ""}${rdelta.toFixed(3)})`);
    process.exit(1);
  }

  console.log(`\nPassed — no regression (threshold=${REGRESSION_THRESHOLD})`);
  console.log(`  precision: ${baseline.aggregate.precision.toFixed(3)} → ${aggregate.precision.toFixed(3)} (${pdelta >= 0 ? "+" : ""}${pdelta.toFixed(3)})`);
  console.log(`  recall:    ${baseline.aggregate.recall.toFixed(3)} → ${aggregate.recall.toFixed(3)} (${rdelta >= 0 ? "+" : ""}${rdelta.toFixed(3)})`);
  process.exit(0);
}

main().catch((err) => {
  console.error("Fatal error:", err);
  process.exit(1);
});
