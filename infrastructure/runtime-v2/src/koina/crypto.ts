// Cryptographic utilities
import { randomBytes } from "node:crypto";

export function generateId(prefix = ""): string {
  const id = randomBytes(12).toString("hex");
  return prefix ? `${prefix}_${id}` : id;
}

export function generateSessionKey(): string {
  return generateId("ses");
}
