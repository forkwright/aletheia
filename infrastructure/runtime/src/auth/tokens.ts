// TODO(unused): scaffolded for spec 3 (Auth & Updates) — not yet integrated into gateway
// JWT signing/verification — HMAC-SHA256, zero external deps
import { createHmac, randomBytes, timingSafeEqual } from "node:crypto";

export interface AccessTokenPayload {
  sub: string; // username
  role: string; // admin | user | readonly
  sid: string; // session ID
  iat: number; // issued at (unix seconds)
  exp: number; // expires at (unix seconds)
}

function base64url(data: string): string {
  return Buffer.from(data).toString("base64url");
}

export function signToken(payload: AccessTokenPayload, secret: string): string {
  const header = base64url(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const body = base64url(JSON.stringify(payload));
  const sig = createHmac("sha256", secret)
    .update(`${header}.${body}`)
    .digest("base64url");
  return `${header}.${body}.${sig}`;
}

export function verifyToken(
  token: string,
  secret: string,
): AccessTokenPayload | null {
  const parts = token.split(".");
  if (parts.length !== 3) return null;

  const [header, body, sig] = parts as [string, string, string];
  const expected = createHmac("sha256", secret)
    .update(`${header}.${body}`)
    .digest("base64url");

  const sigBuf = Buffer.from(sig);
  const expectedBuf = Buffer.from(expected);
  if (sigBuf.length !== expectedBuf.length) return null;
  if (!timingSafeEqual(sigBuf, expectedBuf)) return null;

  try {
    const payload = JSON.parse(
      Buffer.from(body, "base64url").toString(),
    ) as AccessTokenPayload;
    if (payload.exp < Math.floor(Date.now() / 1000)) return null;
    return payload;
  } catch {
    return null;
  }
}

export function generateSessionId(): string {
  return randomBytes(16).toString("hex");
}

export function generateRefreshToken(): string {
  return randomBytes(32).toString("base64url");
}

export function generateSecret(): string {
  return randomBytes(32).toString("base64url");
}
