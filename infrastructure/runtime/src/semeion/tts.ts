// Text-to-speech synthesis — OpenAI TTS primary, Piper local fallback
import { writeFileSync, mkdirSync, unlinkSync, existsSync } from "node:fs";
import { join } from "node:path";
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { tmpdir } from "node:os";
import { createLogger } from "../koina/logger.js";

const log = createLogger("semeion:tts");

const TTS_DIR = process.env["ALETHEIA_TTS_DIR"] ?? join(tmpdir(), "aletheia-tts-" + randomBytes(4).toString("hex"));
const OPENAI_API_KEY = process.env["OPENAI_API_KEY"] ?? "";
const PIPER_BIN = process.env["PIPER_BIN"] ?? "/usr/local/bin/piper";
const PIPER_MODEL = process.env["PIPER_MODEL"] ?? "/usr/local/share/piper/en_US-lessac-medium.onnx";

export interface TtsResult {
  path: string;
  duration?: number;
  engine: "openai" | "piper";
  cleanup: () => void;
}

export interface TtsOptions {
  voice?: string;
  speed?: number;
  engine?: "openai" | "piper" | "auto";
}

export async function synthesize(text: string, opts?: TtsOptions): Promise<TtsResult> {
  mkdirSync(TTS_DIR, { recursive: true });

  const engine = opts?.engine ?? "auto";
  const id = randomBytes(8).toString("hex");

  if (engine === "openai" || (engine === "auto" && OPENAI_API_KEY)) {
    try {
      return await synthesizeOpenAI(text, id, opts);
    } catch (err) {
      log.warn(`OpenAI TTS failed, trying Piper: ${err instanceof Error ? err.message : err}`);
    }
  }

  if (engine === "piper" || engine === "auto") {
    return synthesizePiper(text, id, opts);
  }

  throw new Error("No TTS engine available — set OPENAI_API_KEY or install Piper");
}

async function synthesizeOpenAI(text: string, id: string, opts?: TtsOptions): Promise<TtsResult> {
  const voice = opts?.voice ?? "alloy";
  const speed = opts?.speed ?? 1.0;
  const outPath = join(TTS_DIR, `${id}.mp3`);

  const res = await fetch("https://api.openai.com/v1/audio/speech", {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${OPENAI_API_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      model: "tts-1",
      input: text.slice(0, 4096),
      voice,
      speed,
      response_format: "mp3",
    }),
    signal: AbortSignal.timeout(30000),
  });

  if (!res.ok) {
    const body = await res.text().catch(() => "");
    throw new Error(`OpenAI TTS HTTP ${res.status}: ${body.slice(0, 200)}`);
  }

  const buffer = Buffer.from(await res.arrayBuffer());
  writeFileSync(outPath, buffer);

  log.info(`OpenAI TTS: ${buffer.length} bytes → ${outPath}`);

  return {
    path: outPath,
    engine: "openai",
    cleanup: () => { try { unlinkSync(outPath); } catch {} },
  };
}

function synthesizePiper(text: string, id: string, opts?: TtsOptions): TtsResult {
  if (!existsSync(PIPER_BIN)) {
    throw new Error(`Piper binary not found at ${PIPER_BIN}`);
  }
  if (!existsSync(PIPER_MODEL)) {
    throw new Error(`Piper model not found at ${PIPER_MODEL}`);
  }

  const outPath = join(TTS_DIR, `${id}.wav`);
  const speed = opts?.speed ?? 1.0;

  execFileSync(PIPER_BIN, [
    "--model", PIPER_MODEL,
    "--length_scale", String(1 / speed),
    "--output_file", outPath,
  ], { timeout: 30000, input: text.slice(0, 4096) });

  log.info(`Piper TTS: → ${outPath}`);

  return {
    path: outPath,
    engine: "piper",
    cleanup: () => { try { unlinkSync(outPath); } catch {} },
  };
}

// Cleanup old TTS files (> 1 hour)
export function cleanupTtsFiles(): void {
  try {
    if (!existsSync(TTS_DIR)) return;
    const now = Date.now();
    const { readdirSync, statSync } = require("node:fs") as typeof import("node:fs");
    for (const file of readdirSync(TTS_DIR)) {
      const p = join(TTS_DIR, file);
      try {
        const st = statSync(p);
        if (now - st.mtimeMs > 3600_000) {
          unlinkSync(p);
        }
      } catch {}
    }
  } catch {}
}
