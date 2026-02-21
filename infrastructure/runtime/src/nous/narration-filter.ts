// Narration suppression — reclassifies internal monologue from text to thinking
import { createLogger } from "../koina/logger.js";

const log = createLogger("narration-filter");

const NARRATION_PATTERNS: RegExp[] = [
  /^(?:Let me|I'll|I need to|I'm going to|I should|I want to|I will)\s+(?:check|read|look|search|find|review|examine|analyze|open|save|verify|update|write|edit|create|fetch|query|pull|grab|access|scan|explore|browse|inspect|investigate)/i,
  /^(?:Now (?:let me|I'll|I need to|I'm going to))/i,
  /^(?:First,? (?:let me|I'll|I need to))/i,
  /^(?:OK,? (?:let me|I'll|I need to|so))/i,
  /^(?:Alright,? (?:let me|I'll|I need to))/i,
  /^(?:Looking at|Checking|Reading|Searching|Examining|Reviewing|Analyzing|Opening|Saving|Verifying)/i,
  /^Good (?:call|point|idea|question)[.,]?\s+(?:Let me|I'll|I need to)/i,
  /^(?:Now I (?:have|need|can|should|want))/i,
  /^Let me (?:also|now|first|quickly)\s+(?:check|read|look|search|find|review|examine|analyze|open|save|verify|update|write|edit|create|fetch|query|pull|grab|access|scan|explore|browse|inspect|investigate)/i,
  /^(?:Time to|Going to|About to)\s+/i,
];

export function isNarration(sentence: string): boolean {
  const trimmed = sentence.trim();
  if (trimmed.length < 10 || trimmed.length > 200) return false;
  return NARRATION_PATTERNS.some((p) => p.test(trimmed));
}

type FilterEvent = { type: "text_delta" | "thinking_delta"; text: string };

/**
 * Buffers text_delta chunks at sentence boundaries. Every sentence is
 * classified — narration sentences are reclassified as thinking_delta
 * regardless of position in the response. Non-narration sentences pass
 * through as text_delta.
 */
export class NarrationFilter {
  private buffer = "";
  private suppressed = 0;

  feed(text: string): FilterEvent[] {
    this.buffer += text;
    const events: FilterEvent[] = [];

    const sentencePattern = /[.!?\n]\s+/;
    let match: RegExpExecArray | null;
    while ((match = sentencePattern.exec(this.buffer)) !== null) {
      const sentence = this.buffer.slice(0, match.index + match[0].length);
      this.buffer = this.buffer.slice(match.index + match[0].length);

      if (isNarration(sentence.trim())) {
        events.push({ type: "thinking_delta", text: sentence });
        this.suppressed++;
      } else {
        events.push({ type: "text_delta", text: sentence });
      }
    }

    return events;
  }

  flush(): FilterEvent[] {
    if (!this.buffer) return [];
    const type = isNarration(this.buffer.trim()) ? "thinking_delta" as const : "text_delta" as const;
    if (type === "thinking_delta") this.suppressed++;
    const text = this.buffer;
    this.buffer = "";
    if (this.suppressed > 0) {
      log.debug(`Suppressed ${this.suppressed} narration sentence(s)`);
    }
    return [{ type, text }];
  }
}
