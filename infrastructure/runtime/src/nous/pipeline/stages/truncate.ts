// Per-tool-type result truncation for storage — preserves head + tail with gap notice
const TOOL_RESULT_LIMITS: Record<string, number> = {
  exec: 8000,
  read: 10000,
  grep: 5000,
  find: 3000,
  ls: 2000,
  web_fetch: 8000,
  web_search: 4000,
};

const DEFAULT_LIMIT = 5000;

export function truncateToolResult(toolName: string, result: string): string {
  const limit = TOOL_RESULT_LIMITS[toolName] ?? DEFAULT_LIMIT;
  if (result.length <= limit) return result;

  const headSize = Math.floor(limit * 0.7);
  const tailSize = limit - headSize;
  const omitted = result.length - headSize - tailSize;

  return `${result.slice(0, headSize)}\n\n[... ${omitted} chars omitted ...]\n\n${result.slice(-tailSize)}`;
}
