// Password hashing — scrypt (node:crypto built-in, zero deps)
import { scryptSync, randomBytes, timingSafeEqual } from "node:crypto";

const SALT_LENGTH = 32;
const KEY_LENGTH = 64;
const COST = 16384; // N
const BLOCK_SIZE = 8; // r
const PARALLELISM = 1; // p

export function hashPassword(password: string): string {
  const salt = randomBytes(SALT_LENGTH);
  const derived = scryptSync(password, salt, KEY_LENGTH, {
    N: COST,
    r: BLOCK_SIZE,
    p: PARALLELISM,
  });
  return `$scrypt$N=${COST},r=${BLOCK_SIZE},p=${PARALLELISM}$${salt.toString("base64")}$${derived.toString("base64")}`;
}

export function verifyPassword(password: string, hash: string): boolean {
  const parts = hash.split("$");
  // Format: $scrypt$N=...,r=...,p=...$salt$derived
  if (parts.length !== 5 || parts[1] !== "scrypt") return false;

  const paramStr = parts[2]!;
  const salt = Buffer.from(parts[3]!, "base64");
  const stored = Buffer.from(parts[4]!, "base64");

  const params: Record<string, number> = {};
  for (const pair of paramStr.split(",")) {
    const [key, val] = pair.split("=");
    if (key && val) params[key] = parseInt(val, 10);
  }

  try {
    const derived = scryptSync(password, salt, stored.length, {
      N: params["N"] ?? COST,
      r: params["r"] ?? BLOCK_SIZE,
      p: params["p"] ?? PARALLELISM,
    });
    return timingSafeEqual(stored, derived);
  } catch {
    return false;
  }
}
