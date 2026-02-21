import { describe, it, expect, beforeEach } from "vitest";
import { initEncryption, encrypt, decrypt, isEncrypted, decryptIfNeeded, encryptIfEnabled, isEncryptionReady, getKeySalt } from "./encryption.js";

describe("encryption", () => {
  beforeEach(() => {
    initEncryption("test-passphrase-123", "fixed-salt-for-testing");
  });

  it("encrypts and decrypts round-trip", () => {
    const plaintext = "Hello, this is a secret message!";
    const encrypted = encrypt(plaintext);
    const decrypted = decrypt(encrypted);
    expect(decrypted).toBe(plaintext);
  });

  it("produces different ciphertext for same plaintext (random IV)", () => {
    const plaintext = "same text";
    const a = encrypt(plaintext);
    const b = encrypt(plaintext);
    expect(a).not.toBe(b);
    expect(decrypt(a)).toBe(plaintext);
    expect(decrypt(b)).toBe(plaintext);
  });

  it("detects encrypted content", () => {
    const encrypted = encrypt("test");
    expect(isEncrypted(encrypted)).toBe(true);
    expect(isEncrypted("plain text")).toBe(false);
    expect(isEncrypted('{"key": "value"}')).toBe(false);
  });

  it("decryptIfNeeded passes through plain text", () => {
    expect(decryptIfNeeded("hello world")).toBe("hello world");
  });

  it("decryptIfNeeded decrypts encrypted content", () => {
    const encrypted = encrypt("secret");
    expect(decryptIfNeeded(encrypted)).toBe("secret");
  });

  it("encryptIfEnabled encrypts when ready", () => {
    const result = encryptIfEnabled("test");
    expect(isEncrypted(result)).toBe(true);
    expect(decrypt(result)).toBe("test");
  });

  it("reports encryption ready state", () => {
    expect(isEncryptionReady()).toBe(true);
  });

  it("returns salt", () => {
    expect(getKeySalt()).toBe("fixed-salt-for-testing");
  });

  it("handles unicode content", () => {
    const text = "日本語テスト 🎉 émojis";
    const encrypted = encrypt(text);
    expect(decrypt(encrypted)).toBe(text);
  });

  it("handles large content", () => {
    const text = "x".repeat(100_000);
    const encrypted = encrypt(text);
    expect(decrypt(encrypted)).toBe(text);
  });

  it("handles empty string", () => {
    const encrypted = encrypt("");
    expect(decrypt(encrypted)).toBe("");
  });

  it("fails on tampered ciphertext", () => {
    const encrypted = encrypt("test");
    const payload = JSON.parse(encrypted);
    payload.ct = Buffer.from("tampered").toString("base64");
    expect(() => decrypt(JSON.stringify(payload))).toThrow();
  });
});
