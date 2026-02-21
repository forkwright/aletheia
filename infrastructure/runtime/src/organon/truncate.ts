// Smart tool result truncation
import { estimateTokens } from "../hermeneus/token-counter.js";

const DEFAULT_MAX_TOKENS = 8000;

export interface TruncateOpts {
  maxTokens?: number;
  headRatio?: number;
  format?: "text" | "json" | "lines";
}

export function truncateToolResult(
  result: string,
  opts?: TruncateOpts,
): string {
  const maxTokens = opts?.maxTokens ?? DEFAULT_MAX_TOKENS;
  const tokens = estimateTokens(result);

  if (tokens <= maxTokens) return result;

  const format = opts?.format ?? detectFormat(result);

  switch (format) {
    case "json":
      return truncateJson(result, maxTokens);
    case "lines":
      return truncateLines(result, maxTokens);
    default:
      return truncateText(result, maxTokens, opts?.headRatio ?? 0.7);
  }
}

function detectFormat(text: string): "json" | "lines" | "text" {
  const trimmed = text.trimStart();
  if (trimmed.startsWith("[") || trimmed.startsWith("{")) return "json";

  const lines = text.split("\n");
  if (lines.length > 20) return "lines";

  return "text";
}

function truncateText(
  text: string,
  maxTokens: number,
  headRatio: number,
): string {
  const maxChars = Math.floor(maxTokens * 3.5);
  const headChars = Math.floor(maxChars * headRatio);
  const tailChars = maxChars - headChars;

  const head = text.slice(0, headChars);
  const tail = text.slice(-tailChars);
  const omitted = text.length - headChars - tailChars;

  return `${head}\n\n... [${omitted} chars omitted] ...\n\n${tail}`;
}

function truncateLines(
  text: string,
  maxTokens: number,
): string {
  const lines = text.split("\n");
  const maxChars = Math.floor(maxTokens * 3.5);

  const headLines: string[] = [];
  const tailLines: string[] = [];
  let headChars = 0;
  let tailChars = 0;
  const headBudget = Math.floor(maxChars * 0.6);
  const tailBudget = Math.floor(maxChars * 0.3);

  for (const line of lines) {
    if (headChars + line.length + 1 > headBudget) break;
    headLines.push(line);
    headChars += line.length + 1;
  }

  for (let i = lines.length - 1; i >= headLines.length; i--) {
    const line = lines[i]!;
    if (tailChars + line.length + 1 > tailBudget) break;
    tailLines.unshift(line);
    tailChars += line.length + 1;
  }

  const omitted = lines.length - headLines.length - tailLines.length;

  return [
    ...headLines,
    `\n... [${omitted} lines omitted] ...\n`,
    ...tailLines,
  ].join("\n");
}

function truncateJson(
  text: string,
  maxTokens: number,
): string {
  try {
    const parsed = JSON.parse(text);

    if (Array.isArray(parsed)) {
      const maxItems = Math.max(5, Math.floor(maxTokens / 200));
      if (parsed.length <= maxItems) return text;

      const head = parsed.slice(0, Math.ceil(maxItems * 0.7));
      const tail = parsed.slice(-Math.floor(maxItems * 0.3));

      return JSON.stringify(
        [
          ...head,
          `... [${parsed.length - head.length - tail.length} items omitted] ...`,
          ...tail,
        ],
        null,
        2,
      );
    }

    const maxChars = Math.floor(maxTokens * 3.5);
    const stringified = JSON.stringify(parsed, null, 2);
    if (stringified.length <= maxChars) return stringified;
    return stringified.slice(0, maxChars) + "\n... [truncated]";
  } catch { /* token estimation failed — skip */
    // not valid JSON, fall through
  }

  return truncateText(text, maxTokens, 0.7);
}
