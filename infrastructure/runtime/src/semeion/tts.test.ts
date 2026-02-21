// TTS tests — synthesize, cleanupTtsFiles
import { beforeEach, describe, expect, it, vi } from "vitest";

describe("synthesize", () => {
  beforeEach(() => {
    vi.stubGlobal("fetch", vi.fn());
  });

  it("throws when piper binary missing", async () => {
    // Piper engine requires binary at path
    vi.stubEnv("PIPER_BIN", "/nonexistent/piper");
    vi.stubEnv("PIPER_MODEL", "/nonexistent/model.onnx");
    vi.stubEnv("ALETHEIA_TTS_DIR", "/tmp/aletheia-tts-test");
    vi.stubEnv("OPENAI_API_KEY", "");

    // Dynamic import to pick up env
    const mod = await import("./tts.js");
    // Direct piper call will fail because binary doesn't exist
    await expect(mod.synthesize("hello", { engine: "piper" })).rejects.toThrow("not found");
  });

  it("openai engine calls fetch API", async () => {
    vi.stubEnv("ALETHEIA_TTS_DIR", "/tmp/aletheia-tts-test");
    vi.stubEnv("OPENAI_API_KEY", "sk-test");

    const fakeBuffer = new ArrayBuffer(100);
    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: true,
      arrayBuffer: vi.fn().mockResolvedValue(fakeBuffer),
    });

    const mod = await import("./tts.js");
    const result = await mod.synthesize("hello", { engine: "openai" });
    expect(result.engine).toBe("openai");
    expect(result.path).toContain(".mp3");
    expect(typeof result.cleanup).toBe("function");
    result.cleanup();
  });

  it("openai handles HTTP error and falls back", async () => {
    vi.stubEnv("ALETHEIA_TTS_DIR", "/tmp/aletheia-tts-test");
    vi.stubEnv("OPENAI_API_KEY", "sk-test");
    vi.stubEnv("PIPER_BIN", "/nonexistent/piper");

    (fetch as ReturnType<typeof vi.fn>).mockResolvedValue({
      ok: false,
      status: 401,
      text: vi.fn().mockResolvedValue("Unauthorized"),
    });

    const mod = await import("./tts.js");
    // openai fails → falls back to piper → piper not found → throws
    await expect(mod.synthesize("hello")).rejects.toThrow();
  });
});

describe("cleanupTtsFiles", () => {
  it("is exported and callable", async () => {
    const { cleanupTtsFiles } = await import("./tts.js");
    expect(typeof cleanupTtsFiles).toBe("function");
    // Won't error even when TTS_DIR doesn't exist
    cleanupTtsFiles();
  });
});

describe("TtsResult interface", () => {
  it("module exports synthesize and cleanupTtsFiles", async () => {
    const mod = await import("./tts.js");
    expect(mod.synthesize).toBeDefined();
    expect(mod.cleanupTtsFiles).toBeDefined();
  });
});
