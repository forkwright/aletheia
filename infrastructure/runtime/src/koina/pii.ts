// PII detection and redaction engine
import { createHash } from "node:crypto";
import { createLogger } from "./logger.js";

const log = createLogger("koina:pii");

export type PiiType = "phone" | "email" | "ssn" | "credit_card" | "api_key" | "address";

export type RedactionMode = "mask" | "hash" | "warn";

export interface PiiMatch {
  type: PiiType;
  value: string;
  start: number;
  end: number;
  confidence: number;
}

export interface ScanResult {
  text: string;
  matches: PiiMatch[];
  redacted: number;
}

export interface PiiScanConfig {
  mode: RedactionMode;
  allowlist?: string[] | undefined;
  detectors?: PiiType[] | undefined;
}

const ALL_DETECTORS: PiiType[] = ["phone", "email", "ssn", "credit_card", "api_key", "address"];

// --- Detectors ---

const PHONE_RE = /(?<!\d)(?:\+?1[\s.-]?)?\(?\d{3}\)?[\s.-]?\d{3}[\s.-]?\d{4}(?!\d)/g;
const EMAIL_RE = /\b[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}\b/g;
const SSN_RE = /\b(?!000|666|9\d{2})\d{3}[-\s](?!00)\d{2}[-\s](?!0000)\d{4}\b/g;
const CC_RE = /\b(?:\d[ -]?){13,19}\b/g;

const API_KEY_PREFIXES = [
  /\bsk-ant-[a-zA-Z0-9\-_]{20,}/g,
  /\bsk-[a-zA-Z0-9]{32,}/g,
  /\bghp_[a-zA-Z0-9]{36}/g,
  /\bghs_[a-zA-Z0-9]{36}/g,
  /\bglpat-[a-zA-Z0-9_-]{20,}/g,
  /\bxox[boas]-[a-zA-Z0-9-]{10,}/g,
  /\beyJ[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{20,}\.[a-zA-Z0-9_-]{10,}/g,
];

const ADDRESS_RE = /\b\d{1,5}\s+(?:[A-Z][a-z]+\s+){1,3}(?:St(?:reet)?|Ave(?:nue)?|Blvd|Dr(?:ive)?|Rd|Ct|Ln|Way|Pl(?:ace)?|Cir(?:cle)?|Ter(?:race)?)\b(?:[,\s]+(?:Apt|Suite|Ste|Unit|#)\s*\d+)?/gi;

function detectPhone(text: string): PiiMatch[] {
  return matchAll(text, PHONE_RE, "phone", 0.85);
}

function detectEmail(text: string): PiiMatch[] {
  return matchAll(text, EMAIL_RE, "email", 0.95);
}

function detectSsn(text: string): PiiMatch[] {
  return matchAll(text, SSN_RE, "ssn", 0.95);
}

function detectCreditCard(text: string): PiiMatch[] {
  const matches: PiiMatch[] = [];
  for (const m of text.matchAll(CC_RE)) {
    const raw = m[0]!;
    const digits = raw.replace(/[\s-]/g, "");
    if (digits.length < 13 || digits.length > 19) continue;
    if (!luhn(digits)) continue;
    matches.push({
      type: "credit_card",
      value: raw,
      start: m.index!,
      end: m.index! + raw.length,
      confidence: 0.9,
    });
  }
  return matches;
}

function detectApiKey(text: string): PiiMatch[] {
  const matches: PiiMatch[] = [];
  for (const re of API_KEY_PREFIXES) {
    for (const m of text.matchAll(re)) {
      matches.push({
        type: "api_key",
        value: m[0]!,
        start: m.index!,
        end: m.index! + m[0]!.length,
        confidence: 0.9,
      });
    }
  }
  // High-entropy token detection for unknown prefixes
  const tokenRe = /\b[a-zA-Z0-9\-_]{24,}\b/g;
  for (const m of text.matchAll(tokenRe)) {
    const val = m[0]!;
    if (/^[0-9a-f]+$/i.test(val)) continue; // skip hex-only (git SHA, etc.)
    if (shannonEntropy(val) < 3.5) continue;
    if (matches.some((existing) => existing.start === m.index)) continue;
    matches.push({
      type: "api_key",
      value: val,
      start: m.index!,
      end: m.index! + val.length,
      confidence: 0.7,
    });
  }
  return matches;
}

function detectAddress(text: string): PiiMatch[] {
  return matchAll(text, ADDRESS_RE, "address", 0.6);
}

// --- Helpers ---

function matchAll(text: string, re: RegExp, type: PiiType, confidence: number): PiiMatch[] {
  const matches: PiiMatch[] = [];
  for (const m of text.matchAll(re)) {
    matches.push({
      type,
      value: m[0]!,
      start: m.index!,
      end: m.index! + m[0]!.length,
      confidence,
    });
  }
  return matches;
}

function luhn(digits: string): boolean {
  let sum = 0;
  let alt = false;
  for (let i = digits.length - 1; i >= 0; i--) {
    let n = Number.parseInt(digits[i]!, 10);
    if (alt) {
      n *= 2;
      if (n > 9) n -= 9;
    }
    sum += n;
    alt = !alt;
  }
  return sum % 10 === 0;
}

function shannonEntropy(s: string): number {
  const freq = new Map<string, number>();
  for (const c of s) {
    freq.set(c, (freq.get(c) ?? 0) + 1);
  }
  let entropy = 0;
  for (const count of freq.values()) {
    const p = count / s.length;
    entropy -= p * Math.log2(p);
  }
  return entropy;
}

function matchesAllowlist(value: string, allowlist: string[]): boolean {
  for (const pattern of allowlist) {
    const re = new RegExp(
      "^" + pattern.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*/g, ".*") + "$",
      "i",
    );
    if (re.test(value)) return true;
  }
  return false;
}

function deduplicateSpans(matches: PiiMatch[]): PiiMatch[] {
  if (matches.length <= 1) return matches;
  const result: PiiMatch[] = [];
  let prev = matches[0]!;
  for (let i = 1; i < matches.length; i++) {
    const cur = matches[i]!;
    if (cur.start < prev.end) {
      // Overlapping — keep higher confidence
      prev = cur.confidence > prev.confidence ? cur : prev;
    } else {
      result.push(prev);
      prev = cur;
    }
  }
  result.push(prev);
  return result;
}

function deterministicHash(value: string): string {
  const salt = process.env["ALETHEIA_PII_HASH_SALT"] ?? "aletheia-pii";
  return createHash("sha256")
    .update(salt + value)
    .digest("hex")
    .slice(0, 8);
}

// --- Detector dispatch ---

const DETECTOR_MAP: Record<PiiType, (text: string) => PiiMatch[]> = {
  phone: detectPhone,
  email: detectEmail,
  ssn: detectSsn,
  credit_card: detectCreditCard,
  api_key: detectApiKey,
  address: detectAddress,
};

// --- Core API ---

export function scanText(text: string, config: PiiScanConfig): ScanResult {
  const detectors = config.detectors ?? ALL_DETECTORS;
  const allowlist = config.allowlist ?? [];
  const matches: PiiMatch[] = [];

  for (const type of detectors) {
    const detect = DETECTOR_MAP[type];
    if (!detect) continue;
    for (const match of detect(text)) {
      if (!matchesAllowlist(match.value, allowlist)) {
        matches.push(match);
      }
    }
  }

  matches.sort((a, b) => a.start - b.start);
  const deduped = deduplicateSpans(matches);

  if (config.mode === "warn") {
    if (deduped.length > 0) {
      log.warn(`PII detected (warn mode): ${deduped.map((m) => m.type).join(", ")}`);
    }
    return { text, matches: deduped, redacted: 0 };
  }

  // Apply redactions right-to-left to preserve offsets
  let redacted = text;
  for (const match of [...deduped].reverse()) {
    const replacement =
      config.mode === "hash"
        ? `[${match.type.toUpperCase()}:${deterministicHash(match.value)}]`
        : `[REDACTED:${match.type}]`;
    redacted = redacted.slice(0, match.start) + replacement + redacted.slice(match.end);
  }

  if (deduped.length > 0) {
    log.info(`PII redacted (${config.mode}): ${deduped.length} match(es)`);
  }

  return { text: redacted, matches: deduped, redacted: deduped.length };
}
