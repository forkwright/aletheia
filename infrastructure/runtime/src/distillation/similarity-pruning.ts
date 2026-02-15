// Similarity-based pruning — remove low-information segments before summarization
import { createLogger } from "../koina/logger.js";

const log = createLogger("distillation.pruning");

type SimpleMessage = { role: string; content: string };

export interface PruningResult {
  prunedMessages: SimpleMessage[];
  removedCount: number;
  topicBoundaries: number[];
}

function tokenize(text: string): Set<string> {
  return new Set(
    text
      .toLowerCase()
      .split(/[\s,.:;!?'"()[\]{}]+/)
      .filter((w) => w.length > 2),
  );
}

function jaccardSimilarity(a: Set<string>, b: Set<string>): number {
  if (a.size === 0 && b.size === 0) return 1;
  if (a.size === 0 || b.size === 0) return 0;

  let intersection = 0;
  for (const w of a) {
    if (b.has(w)) intersection++;
  }
  const union = a.size + b.size - intersection;
  return union > 0 ? intersection / union : 0;
}

function windowContent(messages: SimpleMessage[], start: number, size: number): string {
  return messages
    .slice(start, start + size)
    .map((m) => m.content)
    .join(" ");
}

export function pruneBySimilarity(
  messages: SimpleMessage[],
  opts?: {
    windowSize?: number;
    overlapThreshold?: number;
    minMessages?: number;
  },
): PruningResult {
  const windowSize = opts?.windowSize ?? 3;
  const overlapThreshold = opts?.overlapThreshold ?? 0.7;
  const minMessages = opts?.minMessages ?? 10;

  if (messages.length <= minMessages) {
    return { prunedMessages: messages, removedCount: 0, topicBoundaries: [] };
  }

  // Compute similarity between consecutive sliding windows
  const similarities: number[] = [];
  const windowTokens: Set<string>[] = [];

  for (let i = 0; i <= messages.length - windowSize; i++) {
    windowTokens.push(tokenize(windowContent(messages, i, windowSize)));
  }

  for (let i = 0; i < windowTokens.length - 1; i++) {
    similarities.push(jaccardSimilarity(windowTokens[i]!, windowTokens[i + 1]!));
  }

  // Find topic boundaries (low similarity between consecutive windows)
  const topicBoundaries: number[] = [];
  for (let i = 0; i < similarities.length; i++) {
    if (similarities[i]! < 0.2) {
      topicBoundaries.push(i + windowSize);
    }
  }

  // Mark messages in high-overlap regions for removal
  const keepIndices = new Set<number>();

  // Always keep first and last windowSize messages
  for (let i = 0; i < Math.min(windowSize, messages.length); i++) keepIndices.add(i);
  for (let i = Math.max(0, messages.length - windowSize); i < messages.length; i++) keepIndices.add(i);

  // Always keep messages near topic boundaries
  for (const b of topicBoundaries) {
    for (let j = Math.max(0, b - 1); j <= Math.min(messages.length - 1, b + 1); j++) {
      keepIndices.add(j);
    }
  }

  // Always keep user messages (they contain the intent)
  for (let i = 0; i < messages.length; i++) {
    if (messages[i]!.role === "user") keepIndices.add(i);
  }

  // In high-overlap regions, keep only every Nth message
  for (let i = 0; i < similarities.length; i++) {
    if (similarities[i]! >= overlapThreshold) {
      // High overlap — keep first message of the window, skip middle ones
      keepIndices.add(i);
      // Don't add i+1 through i+windowSize-1 (they'll be pruned unless protected above)
    } else {
      // Normal region — keep everything
      for (let j = i; j < Math.min(i + windowSize, messages.length); j++) {
        keepIndices.add(j);
      }
    }
  }

  // Ensure we don't prune below minMessages
  if (keepIndices.size < minMessages) {
    // Just keep everything
    return { prunedMessages: messages, removedCount: 0, topicBoundaries };
  }

  const prunedMessages = messages.filter((_, i) => keepIndices.has(i));
  const removedCount = messages.length - prunedMessages.length;

  if (removedCount > 0) {
    log.info(`Pruned ${removedCount}/${messages.length} messages (${topicBoundaries.length} topic boundaries)`);
  }

  return { prunedMessages, removedCount, topicBoundaries };
}
