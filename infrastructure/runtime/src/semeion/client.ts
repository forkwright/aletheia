// JSON-RPC client for signal-cli HTTP daemon
import { randomUUID } from "node:crypto";
import { createLogger } from "../koina/logger.js";

const log = createLogger("semeion:rpc");

export interface RpcResponse {
  jsonrpc: string;
  result?: unknown;
  error?: { code: number; message: string; data?: unknown };
  id: string;
}

export class SignalClient {
  private baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl.replace(/\/+$/, "");
    if (!this.baseUrl.startsWith("http")) {
      this.baseUrl = `http://${this.baseUrl}`;
    }
  }

  async rpc(method: string, params: Record<string, unknown> = {}): Promise<unknown> {
    const id = randomUUID();
    const body = JSON.stringify({ jsonrpc: "2.0", method, params, id });

    log.debug(`RPC ${method} id=${id}`);

    const res = await fetch(`${this.baseUrl}/api/v1/rpc`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body,
      signal: AbortSignal.timeout(10000),
    });

    if (res.status === 201) return undefined;

    const json = (await res.json()) as RpcResponse;

    if (json.error) {
      throw new Error(`Signal RPC ${json.error.code}: ${json.error.message}`);
    }

    return json.result;
  }

  async send(params: {
    message?: string;
    recipient?: string;
    groupId?: string;
    username?: string;
    account?: string;
    attachments?: string[];
    textStyle?: string[];
  }): Promise<unknown> {
    const rpcParams: Record<string, unknown> = {};

    if (params.message !== null && params.message !== undefined) rpcParams["message"] = params.message;
    if (params.recipient) rpcParams["recipient"] = [params.recipient];
    if (params.groupId) rpcParams["groupId"] = params.groupId;
    if (params.username) rpcParams["username"] = [params.username];
    if (params.account) rpcParams["account"] = params.account;
    if (params.attachments?.length) rpcParams["attachments"] = params.attachments;
    if (params.textStyle?.length) rpcParams["text-style"] = params.textStyle;

    const backoffs = [500, 1000];
    let lastErr: unknown;

    for (let attempt = 0; attempt <= backoffs.length; attempt++) {
      try {
        return await this.rpc("send", rpcParams);
      } catch (err) {
        lastErr = err;
        // Don't retry HTTP 4xx (application errors) — only network/server errors
        if (err instanceof Error && /RPC -?\d+:/.test(err.message)) throw err;
        if (attempt < backoffs.length) {
          log.warn(`Send attempt ${attempt + 1} failed, retrying in ${backoffs[attempt]}ms`);
          await new Promise((r) => setTimeout(r, backoffs[attempt]));
        }
      }
    }

    throw lastErr;
  }

  async sendTyping(params: {
    recipient?: string;
    groupId?: string;
    account?: string;
    stop?: boolean;
  }): Promise<void> {
    const rpcParams: Record<string, unknown> = {};
    if (params.recipient) rpcParams["recipient"] = params.recipient;
    if (params.groupId) rpcParams["groupId"] = params.groupId;
    if (params.account) rpcParams["account"] = params.account;
    if (params.stop) rpcParams["stop"] = true;

    await this.rpc("sendTyping", rpcParams);
  }

  async sendReceipt(params: {
    recipient: string;
    targetTimestamp: number;
    type?: "read" | "viewed";
    account?: string;
  }): Promise<void> {
    await this.rpc("sendReceipt", {
      recipient: params.recipient,
      targetTimestamp: params.targetTimestamp,
      type: params.type ?? "read",
      ...(params.account ? { account: params.account } : {}),
    });
  }

  async sendReaction(params: {
    emoji: string;
    targetTimestamp: number;
    targetAuthor: string;
    recipient?: string;
    groupId?: string;
    account?: string;
    remove?: boolean;
  }): Promise<void> {
    const rpcParams: Record<string, unknown> = {
      emoji: params.emoji,
      targetTimestamp: params.targetTimestamp,
      targetAuthor: params.targetAuthor,
    };
    if (params.recipient) rpcParams["recipients"] = [params.recipient];
    if (params.groupId) rpcParams["groupIds"] = [params.groupId];
    if (params.account) rpcParams["account"] = params.account;
    if (params.remove) rpcParams["remove"] = true;

    await this.rpc("sendReaction", rpcParams);
  }

  async getAttachment(params: {
    id: string;
    account?: string;
  }): Promise<unknown> {
    return this.rpc("getAttachment", {
      id: params.id,
      ...(params.account ? { account: params.account } : {}),
    });
  }

  async health(): Promise<boolean> {
    try {
      const res = await fetch(`${this.baseUrl}/api/v1/check`, {
        signal: AbortSignal.timeout(2000),
      });
      return res.ok;
    } catch {
      return false;
    }
  }
}
