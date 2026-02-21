// Self-observation tool — recent correction patterns
import type { ToolContext, ToolHandler } from "../registry.js";
import type { SessionStore } from "../../mneme/store.js";

export function createRecentCorrectionsTool(store?: SessionStore): ToolHandler {
  return {
    definition: {
      name: "recent_corrections",
      description:
        "Review your recent correction and feedback signals to learn from mistakes.\n\n" +
        "USE WHEN:\n" +
        "- Noticing repeated errors in a domain\n" +
        "- Preparing to attempt a task you've failed at before\n" +
        "- Self-reflecting on interaction patterns\n\n" +
        "DO NOT USE WHEN:\n" +
        "- No relevant correction history exists yet\n\n" +
        "TIPS:\n" +
        "- Shows last N interaction signals with timestamps\n" +
        "- High-confidence corrections are stronger learning signals\n" +
        "- Pattern detection: multiple corrections in same domain = systematic issue",
      input_schema: {
        type: "object",
        properties: {
          limit: {
            type: "number",
            description: "Max signals to return (default 30)",
          },
        },
      },
    },
    async execute(
      input: Record<string, unknown>,
      context: ToolContext,
    ): Promise<string> {
      if (!store) {
        return JSON.stringify({ error: "Store not available" });
      }

      const limit = (input["limit"] as number) || 30;
      const signals = store.getSignalHistory(context.nousId, limit);

      if (signals.length === 0) {
        return JSON.stringify({
          nousId: context.nousId,
          note: "No interaction signals recorded yet",
          signals: [],
        });
      }

      // Group by signal type
      const grouped: Record<string, number> = {};
      for (const s of signals) {
        grouped[s.signal] = (grouped[s.signal] ?? 0) + 1;
      }

      // Find correction clusters (multiple corrections within 1 hour)
      const corrections = signals.filter((s) => s.signal === "correction");
      const clusters: Array<{ count: number; timespan: string }> = [];
      let clusterStart = 0;
      for (let i = 1; i < corrections.length; i++) {
        const prev = new Date(corrections[i - 1]!.createdAt).getTime();
        const curr = new Date(corrections[i]!.createdAt).getTime();
        if (prev - curr > 3600000) {
          if (i - clusterStart > 1) {
            clusters.push({
              count: i - clusterStart,
              timespan: corrections[clusterStart]!.createdAt,
            });
          }
          clusterStart = i;
        }
      }
      if (corrections.length - clusterStart > 1) {
        clusters.push({
          count: corrections.length - clusterStart,
          timespan: corrections[clusterStart]!.createdAt,
        });
      }

      return JSON.stringify({
        nousId: context.nousId,
        summary: grouped,
        correctionClusters: clusters.length > 0 ? clusters : undefined,
        signals: signals.map((s) => ({
          signal: s.signal,
          confidence: s.confidence,
          turn: s.turnSeq,
          at: s.createdAt,
        })),
      });
    },
  };
}
