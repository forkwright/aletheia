// SSRF protection — block requests to internal/private networks
import { lookup } from "node:dns/promises";
import { ToolError } from "../../koina/errors.js";

const BLOCKED_PROTOCOLS = new Set(["file:", "ftp:", "gopher:", "data:"]);

function isPrivateIP(ip: string): boolean {
  if (ip.startsWith("127.")) return true;
  if (ip.startsWith("10.")) return true;
  if (ip.startsWith("192.168.")) return true;
  if (ip === "::1" || ip === "0.0.0.0") return true;
  if (ip.startsWith("169.254.")) return true;
  // 172.16.0.0 - 172.31.255.255
  const m = ip.match(/^172\.(\d+)\./);
  if (m?.[1] && parseInt(m[1], 10) >= 16 && parseInt(m[1], 10) <= 31) return true;
  return false;
}

export async function validateUrl(urlStr: string): Promise<void> {
  const parsed = new URL(urlStr);

  if (BLOCKED_PROTOCOLS.has(parsed.protocol)) {
    throw new ToolError(`Blocked protocol: ${parsed.protocol}`, { code: "TOOL_SSRF_BLOCKED", context: { protocol: parsed.protocol, url: urlStr } });
  }

  const { address } = await lookup(parsed.hostname);
  if (isPrivateIP(address)) {
    throw new ToolError("Blocked: URL resolves to private address", { code: "TOOL_SSRF_BLOCKED", context: { address, url: urlStr } });
  }
}
