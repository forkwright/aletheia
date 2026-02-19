// TODO(unused): scaffolded for spec 3 (Auth & Updates) — not yet integrated into gateway
// Log sanitization — strip user content from error logs
export function sanitizeForLog(text: string, maxLen = 200): string {
  if (!text) return "";
  if (text.length <= maxLen) return text;
  return text.slice(0, 50) + "...[redacted]..." + text.slice(-20);
}

export function sanitizeError(msg: string): string {
  // Strip anything that looks like quoted user content
  return msg.replace(/"[^"]{200,}"/g, '"[content redacted]"');
}
