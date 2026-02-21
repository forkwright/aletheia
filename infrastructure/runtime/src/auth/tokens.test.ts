// JWT token tests
import { describe, expect, it } from "vitest";
import {
  generateRefreshToken,
  generateSecret,
  generateSessionId,
  signToken,
  verifyToken,
  type AccessTokenPayload,
} from "./tokens.js";

const SECRET = "test-secret-key-for-unit-tests";

function makePayload(overrides?: Partial<AccessTokenPayload>): AccessTokenPayload {
  return {
    sub: "testuser",
    role: "admin",
    sid: "ses_123",
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + 900,
    ...overrides,
  };
}

describe("signToken + verifyToken", () => {
  it("round-trips a valid payload", () => {
    const payload = makePayload();
    const token = signToken(payload, SECRET);
    const result = verifyToken(token, SECRET);
    expect(result).toEqual(payload);
  });

  it("produces a 3-part JWT string", () => {
    const token = signToken(makePayload(), SECRET);
    expect(token.split(".")).toHaveLength(3);
  });

  it("returns null for wrong secret", () => {
    const token = signToken(makePayload(), SECRET);
    expect(verifyToken(token, "wrong-secret")).toBeNull();
  });

  it("returns null for expired token", () => {
    const payload = makePayload({ exp: Math.floor(Date.now() / 1000) - 60 });
    const token = signToken(payload, SECRET);
    expect(verifyToken(token, SECRET)).toBeNull();
  });

  it("returns null for malformed token (2 parts)", () => {
    expect(verifyToken("header.body", SECRET)).toBeNull();
  });

  it("returns null for malformed token (4 parts)", () => {
    expect(verifyToken("a.b.c.d", SECRET)).toBeNull();
  });

  it("returns null for tampered payload", () => {
    const token = signToken(makePayload(), SECRET);
    const parts = token.split(".");
    // Tamper with the body
    parts[1] = Buffer.from(JSON.stringify({ sub: "hacker", role: "admin", sid: "x", iat: 0, exp: 9999999999 })).toString("base64url");
    expect(verifyToken(parts.join("."), SECRET)).toBeNull();
  });
});

describe("generators", () => {
  it("generateSessionId returns 32-char hex", () => {
    const id = generateSessionId();
    expect(id).toMatch(/^[0-9a-f]{32}$/);
  });

  it("generateSessionId produces unique values", () => {
    const a = generateSessionId();
    const b = generateSessionId();
    expect(a).not.toBe(b);
  });

  it("generateRefreshToken returns 43-char base64url", () => {
    const token = generateRefreshToken();
    expect(token).toMatch(/^[A-Za-z0-9_-]{43}$/);
  });

  it("generateSecret returns 43-char base64url", () => {
    const secret = generateSecret();
    expect(secret).toMatch(/^[A-Za-z0-9_-]{43}$/);
  });
});
