// Slack mrkdwn ↔ Markdown conversion (Spec 34, Phase 3)
//
// Slack uses "mrkdwn" which is similar to Markdown but has key differences:
//   - Bold: *text* (same)
//   - Italic: _text_ (same)
//   - Strikethrough: ~text~ (same)
//   - Links: <url|label> (different from [label](url))
//   - User mentions: <@U1234>
//   - Channel mentions: <#C1234|name>
//   - Special chars &, <, > must be escaped as &amp; &lt; &gt;
//   - Code blocks: ```text``` (same)
//   - Blockquotes: > text (same)
//
// Reference: OpenClaw src/slack/format.ts — IR-based approach.
// We use a simpler regex pipeline since we don't have their markdown IR layer.
// This handles the 95% case; edge cases can be refined.

// ---------------------------------------------------------------------------
// Markdown → Slack mrkdwn (outbound)
// ---------------------------------------------------------------------------

const SLACK_TEXT_LIMIT = 4000;

/** Characters that need escaping in Slack mrkdwn */
function escapeSlackChars(text: string): string {
  // Don't escape inside code blocks/spans — handled separately
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

/**
 * Convert Markdown to Slack mrkdwn format.
 *
 * Handles: links, bold, italic, strikethrough, code, blockquotes, headers, lists.
 * Preserves code blocks verbatim (only escaping &<> inside them).
 */
export function markdownToMrkdwn(markdown: string): string {
  if (!markdown) return "";

  // Extract code blocks first to protect them from other transforms
  const codeBlocks: string[] = [];
  let text = markdown.replace(/```(\w*)\n?([\s\S]*?)```/g, (_match, _lang, code) => {
    const idx = codeBlocks.length;
    // Escape &<> inside code but preserve everything else
    codeBlocks.push("```\n" + escapeSlackChars(code.trimEnd()) + "\n```");
    return `\x00CODEBLOCK${idx}\x00`;
  });

  // Extract inline code to protect from transforms
  const inlineCode: string[] = [];
  text = text.replace(/`([^`\n]+)`/g, (_match, code) => {
    const idx = inlineCode.length;
    inlineCode.push("`" + escapeSlackChars(code) + "`");
    return `\x00INLINE${idx}\x00`;
  });

  // Escape special chars in remaining text
  text = escapeSlackChars(text);

  // Convert Markdown links [label](url) → Slack <url|label>
  text = text.replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, label, url) => {
    // Unescape the URL (we escaped &<> above but URLs need raw chars)
    const rawUrl = url
      .replace(/&amp;/g, "&")
      .replace(/&lt;/g, "<")
      .replace(/&gt;/g, ">");
    return `<${rawUrl}|${label}>`;
  });

  // Headers → bold (Slack has no header syntax)
  text = text.replace(/^#{1,6}\s+(.+)$/gm, "*$1*");

  // Blockquotes (already same syntax, but need to handle nested)
  // Markdown: > text  →  Slack: > text (same, but ensure &gt; isn't produced)
  text = text.replace(/^&gt;\s?/gm, "> ");

  // Horizontal rules → separator
  text = text.replace(/^(-{3,}|_{3,}|\*{3,})$/gm, "───");

  // Restore code blocks and inline code
  text = text.replace(/\x00CODEBLOCK(\d+)\x00/g, (_m, idx: string) => codeBlocks[Number(idx)] ?? "");
  text = text.replace(/\x00INLINE(\d+)\x00/g, (_m, idx: string) => inlineCode[Number(idx)] ?? "");

  return text.trim();
}

/**
 * Chunk a mrkdwn string at Slack's 4000-char limit.
 * Tries to break at paragraph boundaries, then newlines, then hard-cuts.
 */
export function chunkMrkdwn(text: string, limit = SLACK_TEXT_LIMIT): string[] {
  if (text.length <= limit) return [text];

  const chunks: string[] = [];
  let remaining = text;

  while (remaining.length > limit) {
    // Try to break at double-newline (paragraph boundary)
    let breakIdx = remaining.lastIndexOf("\n\n", limit);

    // Fall back to single newline
    if (breakIdx < Math.floor(limit * 0.3)) {
      breakIdx = remaining.lastIndexOf("\n", limit);
    }

    // Hard cut as last resort
    if (breakIdx < Math.floor(limit * 0.3)) {
      breakIdx = limit;
    }

    chunks.push(remaining.slice(0, breakIdx).trimEnd());
    remaining = remaining.slice(breakIdx).trimStart();
  }

  if (remaining.length > 0) {
    chunks.push(remaining);
  }

  return chunks;
}

// ---------------------------------------------------------------------------
// Slack mrkdwn → Markdown (inbound)
// ---------------------------------------------------------------------------

/**
 * Convert Slack mrkdwn to standard Markdown.
 *
 * Handles: links, user/channel mentions, entity escapes.
 * Bold/italic/strike/code syntax is already compatible.
 */
export function mrkdwnToMarkdown(mrkdwn: string): string {
  if (!mrkdwn) return "";

  let text = mrkdwn;

  // User mentions <@U1234> → @U1234 (before generic link transform)
  text = text.replace(/<@([A-Z0-9]+)>/gi, "@$1");

  // Channel mentions <#C1234|name> → #name (before generic link transform)
  text = text.replace(/<#[A-Z0-9]+\|([^>]+)>/gi, "#$1");

  // Channel mentions without name <#C1234> → #C1234
  text = text.replace(/<#([A-Z0-9]+)>/gi, "#$1");

  // Special commands <!here>, <!channel>, <!everyone> (before generic link)
  text = text.replace(/<!(\w+)>/g, "@$1");

  // Slack links <url|label> → [label](url)
  text = text.replace(/<([^|>]+)\|([^>]+)>/g, "[$2]($1)");

  // Bare Slack URLs <url> → url
  text = text.replace(/<(https?:\/\/[^>]+)>/g, "$1");

  // Unescape HTML entities (Slack sends these)
  text = text.replace(/&amp;/g, "&");
  text = text.replace(/&lt;/g, "<");
  text = text.replace(/&gt;/g, ">");

  return text.trim();
}

/**
 * Strip bot mention from the beginning of a message.
 * Slack sends mentions as <@BOTID> at the start of app_mention events.
 */
export function stripBotMention(text: string, botUserId: string): string {
  // Pattern: <@BOTID> optionally followed by whitespace/colon
  const mentionPattern = new RegExp(`^<@${botUserId}>[:,\\s]*`, "i");
  return text.replace(mentionPattern, "").trim();
}
