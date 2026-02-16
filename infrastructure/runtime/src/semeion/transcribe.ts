// Audio transcription via whisper.cpp
import { execFile } from "node:child_process";
import { writeFileSync, unlinkSync, existsSync, mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createLogger } from "../koina/logger.js";

const log = createLogger("semeion.transcribe");

const WHISPER_BINARY = process.env["WHISPER_BINARY"] ?? "whisper-cpp";
const WHISPER_MODEL = process.env["WHISPER_MODEL_PATH"] ?? "";
const TIMEOUT_MS = 60_000;

export function isWhisperAvailable(): boolean {
  if (!WHISPER_MODEL || !existsSync(WHISPER_MODEL)) {
    return false;
  }
  try {
    const { execFileSync } = require("node:child_process") as typeof import("node:child_process");
    execFileSync("which", [WHISPER_BINARY], { timeout: 5000 });
    return true;
  } catch {
    return false;
  }
}

export async function transcribeAudio(
  base64Data: string,
  contentType: string,
): Promise<string | null> {
  if (!WHISPER_MODEL) {
    log.debug("WHISPER_MODEL_PATH not set — skipping transcription");
    return null;
  }

  // Write base64 audio to temp file
  const ext = contentType.includes("ogg") ? ".ogg"
    : contentType.includes("mp4") ? ".m4a"
    : contentType.includes("wav") ? ".wav"
    : contentType.includes("mpeg") ? ".mp3"
    : ".ogg"; // Signal default is OGG/Opus

  const tmpDir = mkdtempSync(join(tmpdir(), "aletheia-audio-"));
  const tmpPath = join(tmpDir, `audio${ext}`);
  const wavPath = tmpPath.replace(/\.[^.]+$/, ".wav");

  try {
    writeFileSync(tmpPath, Buffer.from(base64Data, "base64"));

    // Convert to WAV (whisper.cpp requires 16kHz WAV)
    await execAsync("ffmpeg", [
      "-i", tmpPath,
      "-ar", "16000",
      "-ac", "1",
      "-f", "wav",
      "-y", wavPath,
    ]);

    // Run whisper.cpp
    const output = await execAsync(WHISPER_BINARY, [
      "-m", WHISPER_MODEL,
      "-f", wavPath,
      "--no-timestamps",
      "-l", "en",
    ]);

    const transcript = output.trim();
    if (!transcript) return null;

    log.info(`Transcribed audio: ${transcript.length} chars`);
    return transcript;
  } catch (err) {
    log.warn(`Transcription failed: ${err instanceof Error ? err.message : err}`);
    return null;
  } finally {
    try { unlinkSync(tmpPath); } catch { /* cleanup */ }
    try { unlinkSync(wavPath); } catch { /* cleanup */ }
    try {
      const { rmSync } = require("node:fs") as typeof import("node:fs");
      rmSync(tmpDir, { recursive: true, force: true });
    } catch { /* cleanup */ }
  }
}

function execAsync(cmd: string, args: string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(cmd, args, { timeout: TIMEOUT_MS, maxBuffer: 10 * 1024 * 1024 }, (err, stdout, stderr) => {
      if (err) {
        reject(new Error(`${cmd} failed: ${stderr || err.message}`));
      } else {
        resolve(stdout);
      }
    });
  });
}
