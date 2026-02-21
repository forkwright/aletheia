// Distillation workspace flush — write summary + extraction to agent memory file
import { join } from "node:path";
import { appendFileSync, existsSync, mkdirSync } from "node:fs";
import { createLogger } from "../koina/logger.js";
import type { ExtractionResult } from "./extract.js";

const log = createLogger("distillation:workspace");

export interface WorkspaceFlushOpts {
  workspace: string;
  nousId: string;
  sessionId: string;
  distillationNumber: number;
  summary: string;
  extraction: ExtractionResult;
}

export interface WorkspaceFlushResult {
  written: boolean;
  path: string;
  error?: string;
}

export function flushToWorkspace(opts: WorkspaceFlushOpts): WorkspaceFlushResult {
  const now = new Date();
  const dateStr = now.toISOString().slice(0, 10);
  const timeStr = now.toLocaleTimeString("en-US", {
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
    timeZone: process.env["TZ"] ?? "UTC",
  });

  const memoryDir = join(opts.workspace, "memory");
  const filePath = join(memoryDir, `${dateStr}.md`);

  try {
    if (!existsSync(memoryDir)) {
      mkdirSync(memoryDir, { recursive: true });
    }

    const sections: string[] = [];

    if (!existsSync(filePath)) {
      sections.push(`# Memory — ${dateStr}\n`);
    }

    sections.push(`\n---\n`);
    sections.push(
      `## Distillation #${opts.distillationNumber} — ${timeStr} (session: ${opts.sessionId.slice(0, 12)})\n`,
    );

    sections.push(`### Summary\n`);
    sections.push(opts.summary.trim());
    sections.push(``);

    const ext = opts.extraction;
    const hasFacts = ext.facts.length > 0;
    const hasDecisions = ext.decisions.length > 0;
    const hasOpen = ext.openItems.length > 0;
    const hasContradictions = ext.contradictions.length > 0;

    if (hasFacts || hasDecisions || hasOpen) {
      sections.push(`### Extracted`);
      sections.push(`- **Facts:** ${ext.facts.length}`);
      sections.push(`- **Decisions:** ${ext.decisions.length}`);
      sections.push(`- **Open Items:** ${ext.openItems.length}`);
      if (hasContradictions) {
        sections.push(`- **Contradictions:** ${ext.contradictions.length}`);
      }
      sections.push(``);
    }

    if (hasFacts) {
      sections.push(`#### Key Facts`);
      const capped = ext.facts.slice(0, 20);
      for (const fact of capped) {
        sections.push(`- ${fact}`);
      }
      if (ext.facts.length > 20) {
        sections.push(`- ... and ${ext.facts.length - 20} more`);
      }
      sections.push(``);
    }

    if (hasDecisions) {
      sections.push(`#### Decisions`);
      for (const d of ext.decisions) {
        sections.push(`- ${d}`);
      }
      sections.push(``);
    }

    if (hasOpen) {
      sections.push(`#### Open Items`);
      for (const item of ext.openItems) {
        sections.push(`- ${item}`);
      }
      sections.push(``);
    }

    if (hasContradictions) {
      sections.push(`#### Contradictions`);
      for (const c of ext.contradictions) {
        sections.push(`- ${c}`);
      }
      sections.push(``);
    }

    const content = sections.join("\n") + "\n";
    appendFileSync(filePath, content, "utf-8");

    log.info(
      `Workspace memory written: ${filePath} (distillation #${opts.distillationNumber})`,
    );
    return { written: true, path: filePath };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    log.error(`Workspace memory flush failed for ${opts.nousId}: ${msg}`);
    return { written: false, path: filePath, error: msg };
  }
}
