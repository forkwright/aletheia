// Signal channel provider — wraps semeion into the agora ChannelProvider interface (Spec 34)
//
// This is a thin adapter. All real Signal logic stays in semeion's existing files.
// The provider owns the lifecycle (daemon, client, listener) and exposes send/probe.

import { createLogger } from "../koina/logger.js";
import type {
  ChannelCapabilities,
  ChannelContext,
  ChannelProbeResult,
  ChannelProvider,
  ChannelSendParams,
  ChannelSendResult,
} from "../agora/types.js";
import { SignalClient } from "./client.js";
import {
  type DaemonHandle,
  daemonOptsFromConfig,
  spawnDaemon,
  waitForReady,
} from "./daemon.js";
import { startListener, type ListenerOpts } from "./listener.js";
import { initSenderPii, parseTarget, sendMessage, type SendTarget } from "./sender.js";
import type { AletheiaConfig } from "../taxis/schema.js";
import type { SkillRegistry } from "../organon/skills.js";
import type { CommandRegistry } from "./commands.js";

const log = createLogger("semeion:provider");

export interface SignalProviderOpts {
  config: AletheiaConfig;
  commands?: CommandRegistry;
  skills?: SkillRegistry | null;
  onStatusRequest?: (client: SignalClient, target: SendTarget) => Promise<void>;
}

/**
 * SignalChannelProvider — adapts semeion to ChannelProvider.
 *
 * On start():
 *   - Spawns signal-cli daemons for each configured account
 *   - Creates SignalClient instances
 *   - Starts SSE listeners that dispatch to the nous pipeline via ctx.dispatch()
 *
 * On send():
 *   - Parses the target format and routes to the correct SignalClient
 *
 * On stop():
 *   - Aborts the SSE listeners
 *   - Stops signal-cli daemons
 */
export class SignalChannelProvider implements ChannelProvider {
  readonly id = "signal";
  readonly name = "Signal";
  readonly capabilities: ChannelCapabilities = {
    threads: false,
    reactions: true,
    typing: true,
    media: true,
    streaming: false,
    richFormatting: false,
    maxTextLength: 2000,
  };

  private readonly config: AletheiaConfig;
  private commandRegistryLocal: CommandRegistry | undefined;
  private skillsLocal: SkillRegistry | null | undefined;
  private onStatusRequestFn: ((client: SignalClient, target: SendTarget) => Promise<void>) | undefined;

  private daemons: DaemonHandle[] = [];
  private clients = new Map<string, SignalClient>();
  private abortController: AbortController | undefined;
  private defaultAccountPhone: string | undefined;
  private defaultClient: SignalClient | undefined;

  constructor(opts: SignalProviderOpts) {
    this.config = opts.config;
    this.commandRegistryLocal = opts.commands;
    this.skillsLocal = opts.skills;
    this.onStatusRequestFn = opts.onStatusRequest;
  }

  async start(ctx: ChannelContext): Promise<void> {
    if (!this.config.channels.signal.enabled) {
      log.info("Signal channel disabled in config");
      return;
    }

    this.abortController = new AbortController();

    // Chain to the parent abort signal
    ctx.abortSignal.addEventListener("abort", () => this.abortController?.abort(), { once: true });

    initSenderPii(this.config.privacy?.pii);

    // Collect bound group IDs from bindings so the listener can allow them
    const boundGroupIds = new Set<string>();
    for (const binding of this.config.bindings) {
      if (binding.match.peer?.kind === "group" && binding.match.peer.id) {
        boundGroupIds.add(binding.match.peer.id);
      }
    }

    const commands = this.commandRegistryLocal ?? ctx.commands;
    const skills = this.skillsLocal ?? null;

    for (const [accountId, account] of Object.entries(
      this.config.channels.signal.accounts,
    )) {
      if (!account.enabled) continue;

      const httpUrl =
        account.httpUrl ??
        `http://${account.httpHost}:${account.httpPort}`;

      // Spawn daemon if auto-start enabled
      if (account.autoStart) {
        const daemonOpts = daemonOptsFromConfig(accountId, account);
        const handle = spawnDaemon(daemonOpts);
        this.daemons.push(handle);

        try {
          await waitForReady(handle.baseUrl);
        } catch (error) {
          log.error(
            `Signal daemon for ${accountId} failed to start: ${error instanceof Error ? error.message : error}`,
          );
          continue;
        }
      }

      const client = new SignalClient(httpUrl);
      this.clients.set(accountId, client);

      // Track default (first) account for outbound
      if (!this.defaultClient) {
        this.defaultClient = client;
        this.defaultAccountPhone = account.account ?? accountId;
      }

      // Start SSE listener — dispatches to nous via ctx.manager.handleMessage
      const listenerOpts: ListenerOpts = {
        accountId,
        account,
        manager: ctx.manager,
        client,
        baseUrl: httpUrl,
        abortSignal: this.abortController.signal,
        boundGroupIds,
        store: ctx.store,
        config: this.config,
        get watchdog() { return ctx.watchdog ?? null; },
      };

      // Conditionally assign optional fields to avoid exactOptionalPropertyTypes issues
      if (commands) listenerOpts.commands = commands;
      if (skills) listenerOpts.skills = skills;

      if (this.onStatusRequestFn) {
        const statusHandler = this.onStatusRequestFn;
        listenerOpts.onStatusRequest = async (target) => {
          await statusHandler(client, target);
        };
      }

      // startListener is fire-and-forget (it loops internally with reconnect)
      startListener(listenerOpts);

      log.info(`Signal account ${accountId} active at ${httpUrl}`);
    }

    log.info(`Signal provider started: ${this.clients.size} account(s)`);
  }

  async send(params: ChannelSendParams): Promise<ChannelSendResult> {
    const client = this.resolveClient(params.accountId);
    if (!client) {
      return { sent: false, error: "No Signal client available" };
    }

    const account = this.resolveAccountPhone(params.accountId);
    const target = parseTarget(params.to, account);

    try {
      const sendOpts: { markdown?: boolean; attachments?: string[] } = {};
      if (params.markdown !== undefined) sendOpts.markdown = params.markdown;
      if (params.attachments) sendOpts.attachments = params.attachments;

      await sendMessage(client, target, params.text, sendOpts);
      return { sent: true };
    } catch (error) {
      return {
        sent: false,
        error: `Signal send failed: ${error instanceof Error ? error.message : error}`,
      };
    }
  }

  async sendTyping(to: string, accountId?: string, stop = false): Promise<void> {
    const client = this.resolveClient(accountId);
    if (!client) return;

    const account = this.resolveAccountPhone(accountId);
    const target = parseTarget(to, account);

    const { sendTyping: sendTypingFn } = await import("./sender.js");
    await sendTypingFn(client, target, stop);
  }

  async stop(): Promise<void> {
    this.abortController?.abort();
    for (const daemon of this.daemons) {
      daemon.stop();
    }
    this.daemons = [];
    this.clients.clear();
    this.defaultClient = undefined;
    this.defaultAccountPhone = undefined;
    log.info("Signal provider stopped");
  }

  async probe(): Promise<ChannelProbeResult> {
    if (this.clients.size === 0) {
      return { ok: false, error: "No Signal clients configured" };
    }

    // Probe each client via health endpoint
    const accountResults: Record<string, boolean> = {};
    let anyOk = false;

    for (const [accountId, client] of this.clients) {
      try {
        const ok = await client.health();
        accountResults[accountId] = ok;
        if (ok) anyOk = true;
      } catch {
        accountResults[accountId] = false;
      }
    }

    return {
      ok: anyOk,
      details: { accounts: accountResults },
    };
  }

  // --- Accessors for legacy code that needs raw client/account ---

  /** Get the default SignalClient (first configured account) */
  getDefaultClient(): SignalClient | undefined {
    return this.defaultClient;
  }

  /** Get the default account phone number */
  getDefaultAccountPhone(): string | undefined {
    return this.defaultAccountPhone;
  }

  /** Get a specific SignalClient by account ID */
  getClient(accountId: string): SignalClient | undefined {
    return this.clients.get(accountId);
  }

  /** Check if any Signal clients are available */
  get hasClients(): boolean {
    return this.clients.size > 0;
  }

  // --- Internal ---

  private resolveClient(accountId?: string): SignalClient | undefined {
    if (accountId) return this.clients.get(accountId);
    return this.defaultClient;
  }

  private resolveAccountPhone(accountId?: string): string {
    if (accountId) {
      const account = this.config.channels.signal.accounts[accountId];
      return account?.account ?? accountId;
    }
    return this.defaultAccountPhone ?? "";
  }
}
