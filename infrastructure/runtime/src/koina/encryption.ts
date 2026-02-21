// AES-256-GCM encryption for data at rest
import { createCipheriv, createDecipheriv, pbkdf2Sync, randomBytes } from "node:crypto";
import { createLogger } from "./logger.js";

const log = createLogger("encryption");

const ALGORITHM = "aes-256-gcm";
const IV_LENGTH = 12;
const TAG_LENGTH = 16;
const SALT_LENGTH = 32;
const PBKDF2_ITERATIONS = 100_000;
const KEY_LENGTH = 32;

export interface EncryptedPayload {
  v: 1;
  ct: string; // ciphertext (base64)
  iv: string; // initialization vector (base64)
  tag: string; // auth tag (base64)
}

let derivedKey: Buffer | null = null;
let keySalt: string | null = null;

export function initEncryption(passphrase: string, salt?: string): void {
  keySalt = salt ?? randomBytes(SALT_LENGTH).toString("hex");
  derivedKey = pbkdf2Sync(passphrase, keySalt, PBKDF2_ITERATIONS, KEY_LENGTH, "sha256");
  log.info("Encryption initialized");
}

export function getKeySalt(): string | null {
  return keySalt;
}

export function isEncryptionReady(): boolean {
  return derivedKey !== null;
}

export function encrypt(plaintext: string): string {
  if (!derivedKey) throw new Error("Encryption not initialized — call initEncryption first");

  const iv = randomBytes(IV_LENGTH);
  const cipher = createCipheriv(ALGORITHM, derivedKey, iv, { authTagLength: TAG_LENGTH });

  const encrypted = Buffer.concat([
    cipher.update(plaintext, "utf8"),
    cipher.final(),
  ]);
  const tag = cipher.getAuthTag();

  const payload: EncryptedPayload = {
    v: 1,
    ct: encrypted.toString("base64"),
    iv: iv.toString("base64"),
    tag: tag.toString("base64"),
  };

  return JSON.stringify(payload);
}

export function decrypt(encoded: string): string {
  if (!derivedKey) throw new Error("Encryption not initialized — call initEncryption first");

  const payload = JSON.parse(encoded) as EncryptedPayload;
  if (payload.v !== 1) throw new Error(`Unsupported encryption version: ${payload.v}`);

  const iv = Buffer.from(payload.iv, "base64");
  const tag = Buffer.from(payload.tag, "base64");
  const ciphertext = Buffer.from(payload.ct, "base64");

  const decipher = createDecipheriv(ALGORITHM, derivedKey, iv, { authTagLength: TAG_LENGTH });
  decipher.setAuthTag(tag);

  const decrypted = Buffer.concat([
    decipher.update(ciphertext),
    decipher.final(),
  ]);

  return decrypted.toString("utf8");
}

export function isEncrypted(content: string): boolean {
  if (!content.startsWith("{")) return false;
  try {
    const parsed = JSON.parse(content);
    return parsed.v === 1 && typeof parsed.ct === "string" && typeof parsed.iv === "string";
  } catch { /* decryption failed — return null */
    return false;
  }
}

export function decryptIfNeeded(content: string): string {
  if (!isEncryptionReady()) return content;
  if (!isEncrypted(content)) return content;
  return decrypt(content);
}

export function encryptIfEnabled(content: string): string {
  if (!isEncryptionReady()) return content;
  return encrypt(content);
}
