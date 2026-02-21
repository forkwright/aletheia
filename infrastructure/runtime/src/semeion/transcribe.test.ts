// Transcription module tests
import { describe, expect, it } from "vitest";
import { isWhisperAvailable, transcribeAudio } from "./transcribe.js";

describe("transcribe", () => {
  it("isWhisperAvailable returns false without WHISPER_MODEL_PATH", () => {
    // In test env, WHISPER_MODEL_PATH is not set
    const original = process.env["WHISPER_MODEL_PATH"];
    delete process.env["WHISPER_MODEL_PATH"];
    expect(isWhisperAvailable()).toBe(false);
    if (original) process.env["WHISPER_MODEL_PATH"] = original;
  });

  it("transcribeAudio returns null without WHISPER_MODEL_PATH", async () => {
    const original = process.env["WHISPER_MODEL_PATH"];
    delete process.env["WHISPER_MODEL_PATH"];
    const result = await transcribeAudio("dGVzdA==", "audio/ogg");
    expect(result).toBeNull();
    if (original) process.env["WHISPER_MODEL_PATH"] = original;
  });

  it("transcribeAudio returns null with nonexistent model path", async () => {
    const original = process.env["WHISPER_MODEL_PATH"];
    process.env["WHISPER_MODEL_PATH"] = "/nonexistent/model.bin";
    const result = await transcribeAudio("dGVzdA==", "audio/ogg");
    expect(result).toBeNull();
    if (original) {
      process.env["WHISPER_MODEL_PATH"] = original;
    } else {
      delete process.env["WHISPER_MODEL_PATH"];
    }
  });
});
