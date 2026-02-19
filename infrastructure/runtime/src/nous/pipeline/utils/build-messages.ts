// Message builder — converts stored Message[] into API-shaped MessageParam[]
// Handles media blocks, orphaned tool_use repair, consecutive user merge, ephemeral timestamps
import { createLogger } from "../../../koina/logger.js";
import { eventBus } from "../../../koina/event-bus.js";
import type { Message } from "../../../mneme/store.js";
import type {
  ContentBlock,
  ImageBlock,
  MessageParam,
  ToolUseBlock,
  UserContentBlock,
} from "../../../hermeneus/anthropic.js";
import type { MediaAttachment } from "../types.js";

const log = createLogger("pipeline:messages");

function formatEphemeralTimestamp(isoString: string, tz: string = "UTC"): string | null {
  try {
    const d = new Date(isoString);
    if (isNaN(d.getTime())) return null;
    return d.toLocaleString("en-US", {
      timeZone: tz,
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
      hour12: true,
    });
  } catch {
    return null;
  }
}

export function buildMessages(
  history: Message[],
  currentText: string,
  media?: MediaAttachment[],
  tz?: string,
): MessageParam[] {
  const messages: MessageParam[] = [];

  for (let i = 0; i < history.length; i++) {
    const msg = history[i]!;

    if (msg.role === "user") {
      const ts = formatEphemeralTimestamp(msg.createdAt, tz);
      const content = ts ? `[${ts}] ${msg.content}` : msg.content;
      messages.push({ role: "user", content });
    } else if (msg.role === "assistant") {
      try {
        const parsed = JSON.parse(msg.content);
        if (Array.isArray(parsed) && parsed.length > 0 && parsed[0]?.type) {
          // Strip thinking blocks without signatures from history — the API
          // rejects them without the signature field. Blocks with signatures
          // (captured during streaming) are kept for optimal context.
          const filtered = (parsed as Array<{ type: string; signature?: string }>).filter(
            (b) => b.type !== "thinking" || b.signature,
          );
          if (filtered.length > 0) {
            messages.push({ role: "assistant", content: filtered as ContentBlock[] });
          } else {
            messages.push({ role: "assistant", content: "" });
          }
          continue;
        }
      } catch {
        // Not JSON — plain text assistant message
      }
      messages.push({ role: "assistant", content: msg.content });
    } else if (msg.role === "tool_result") {
      const toolResults: UserContentBlock[] = [];
      while (i < history.length && history[i]!.role === "tool_result") {
        const tr = history[i]!;
        toolResults.push({
          type: "tool_result",
          tool_use_id: tr.toolCallId ?? "",
          content: tr.content,
        });
        i++;
      }
      i--;

      const prev = messages[messages.length - 1];
      if (prev?.role === "assistant" && Array.isArray(prev.content)) {
        const toolUseIds = new Set(
          (prev.content as ContentBlock[])
            .filter((b): b is ToolUseBlock => b.type === "tool_use")
            .map((b) => b.id),
        );
        const valid = toolResults.filter((tr) =>
          "tool_use_id" in tr && toolUseIds.has(tr.tool_use_id),
        );
        if (valid.length > 0) {
          messages.push({ role: "user", content: valid });
        } else {
          log.debug("Dropping orphaned tool_results (no matching tool_use)");
        }
      } else {
        log.debug("Dropping orphaned tool_results (no preceding assistant tool_use)");
      }
    }
  }

  // Current message — multimodal if media present
  const hasMedia = media && media.length > 0;
  if (hasMedia) {
    const blocks: UserContentBlock[] = [];

    for (const item of media) {
      let data = item.data;
      const dataUriMatch = data.match(/^data:[^;]+;base64,(.+)$/);
      if (dataUriMatch) data = dataUriMatch[1]!;

      if (/^image\/(jpeg|png|gif|webp)$/i.test(item.contentType)) {
        blocks.push({
          type: "image",
          source: {
            type: "base64",
            media_type: item.contentType as "image/jpeg" | "image/png" | "image/gif" | "image/webp",
            data,
          },
        } as ImageBlock);
      } else if (item.contentType === "application/pdf") {
        blocks.push({
          type: "document",
          source: { type: "base64", media_type: "application/pdf", data },
          ...(item.filename ? { title: item.filename } : {}),
        } as unknown as UserContentBlock);
      } else if (item.contentType.startsWith("text/")) {
        try {
          const decoded = Buffer.from(data, "base64").toString("utf-8");
          const label = item.filename ? `[File: ${item.filename}]` : "[Text file]";
          blocks.push({ type: "text", text: `${label}\n\n${decoded}` });
        } catch {
          blocks.push({ type: "text", text: `[Could not decode text file: ${item.filename ?? "unknown"}]` });
        }
      } else {
        blocks.push({
          type: "text",
          text: `[Attachment: ${item.filename ?? "file"} (${item.contentType}) — unsupported for inline viewing]`,
        });
      }
    }

    log.info(`Including ${blocks.length} content block(s) from media (${blocks.map(b => b.type).join(", ")})`);
    blocks.push({ type: "text", text: currentText });
    messages.push({ role: "user", content: blocks });
  } else {
    messages.push({ role: "user", content: currentText });
  }

  // Repair orphaned tool_use blocks
  // First, build a global set of ALL tool_result IDs already present in the message array.
  // This prevents creating synthetic results for tool_use blocks that already have a
  // real result somewhere in the history (even if not immediately adjacent).
  const globalAnsweredIds = new Set<string>();
  for (const m of messages) {
    if (m.role !== "user" || !Array.isArray(m.content)) continue;
    for (const block of m.content as UserContentBlock[]) {
      if ("tool_use_id" in block) globalAnsweredIds.add(block.tool_use_id);
    }
  }

  for (let j = 0; j < messages.length; j++) {
    const msg = messages[j]!;
    if (msg.role !== "assistant" || !Array.isArray(msg.content)) continue;
    const toolUseBlocks = (msg.content as ContentBlock[]).filter(
      (b): b is ToolUseBlock => b.type === "tool_use",
    );
    if (toolUseBlocks.length === 0) continue;

    // Check both the immediately-next message and the global set
    const next = messages[j + 1];
    const localAnsweredIds = new Set<string>();
    if (next?.role === "user" && Array.isArray(next.content)) {
      for (const block of next.content as UserContentBlock[]) {
        if ("tool_use_id" in block) localAnsweredIds.add(block.tool_use_id);
      }
    }
    const orphaned = toolUseBlocks.filter((b) => !localAnsweredIds.has(b.id) && !globalAnsweredIds.has(b.id));
    if (orphaned.length === 0) continue;

    const details = orphaned.map(b => `${b.name ?? "unknown"}(${b.id})`).join(", ");
    log.warn(`Repairing ${orphaned.length} orphaned tool_use block(s): ${details}`);
    eventBus.emit("history:orphan_repair", {
      count: orphaned.length,
      tools: orphaned.map(b => b.name ?? "unknown"),
    });
    const syntheticResults: UserContentBlock[] = orphaned.map((b) => ({
      type: "tool_result" as const,
      tool_use_id: b.id,
      content: `Error: Tool "${b.name ?? "unknown"}" execution interrupted — service restarted mid-turn.`,
    }));

    // Track synthetic IDs so we don't duplicate them for later assistant messages
    for (const sr of syntheticResults) {
      if ("tool_use_id" in sr) globalAnsweredIds.add(sr.tool_use_id);
    }

    if (next?.role === "user" && Array.isArray(next.content)) {
      (next.content as UserContentBlock[]).unshift(...syntheticResults);
    } else {
      messages.splice(j + 1, 0, { role: "user", content: syntheticResults });
    }
  }

  // Deduplicate tool_results — Anthropic rejects messages with multiple tool_result
  // blocks sharing the same tool_use_id (400: "each tool_use must have a single result")
  const seenToolResultIds = new Set<string>();
  for (const m of messages) {
    if (m.role !== "user" || !Array.isArray(m.content)) continue;
    const deduped = (m.content as UserContentBlock[]).filter((block) => {
      if (!("tool_use_id" in block)) return true;
      const id = block.tool_use_id;
      if (seenToolResultIds.has(id)) {
        log.warn(`Dropping duplicate tool_result for ${id}`);
        return false;
      }
      seenToolResultIds.add(id);
      return true;
    });
    (m as { content: UserContentBlock[] }).content = deduped;
  }

  // Merge consecutive user messages to prevent Anthropic 400 errors
  const merged: MessageParam[] = [];
  for (const m of messages) {
    const prev = merged[merged.length - 1];
    if (
      prev &&
      prev.role === "user" &&
      m.role === "user" &&
      typeof prev.content === "string" &&
      typeof m.content === "string"
    ) {
      prev.content = prev.content + "\n\n" + m.content;
    } else {
      merged.push({ ...m });
    }
  }

  return merged;
}
