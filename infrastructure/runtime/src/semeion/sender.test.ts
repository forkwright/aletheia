// Sender utility tests
import { describe, it, expect } from "vitest";
import { parseTarget } from "./sender.js";

describe("parseTarget", () => {
  it("parses group: prefix", () => {
    const target = parseTarget("group:abc123", "acct");
    expect(target.groupId).toBe("abc123");
    expect(target.recipient).toBeUndefined();
    expect(target.account).toBe("acct");
  });

  it("parses u: prefix as username", () => {
    const target = parseTarget("u:john.doe", "acct");
    expect(target.username).toBe("john.doe");
  });

  it("parses username: prefix", () => {
    const target = parseTarget("username:jane", "acct");
    expect(target.username).toBe("jane");
  });

  it("strips signal: prefix for phone numbers", () => {
    const target = parseTarget("signal:+15551234567", "acct");
    expect(target.recipient).toBe("+15551234567");
  });

  it("treats plain phone as recipient", () => {
    const target = parseTarget("+15551234567", "acct");
    expect(target.recipient).toBe("+15551234567");
  });

  it("always includes account", () => {
    const target = parseTarget("+1234", "myaccount");
    expect(target.account).toBe("myaccount");
  });
});
