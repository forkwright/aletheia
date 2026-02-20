// MCP client manager — connects to configured MCP servers, discovers tools, registers into ToolRegistry

import { ConfigError } from "../koina/errors.js";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import { SSEClientTransport } from "@modelcontextprotocol/sdk/client/sse.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import type { Transport } from "@modelcontextprotocol/sdk/shared/transport.js";
import type { ToolRegistry, ToolHandler, ToolContext } from "./registry.js";
import { createLogger } from "../koina/logger.js";
import { getVersion } from "../version.js";

const log = createLogger("mcp-client");

export interface McpServerConfig {
  transport: "stdio" | "http" | "sse";
  command?: string;
  args: string[];
  env: Record<string, string>;
  url?: string;
  headers: Record<string, string>;
  timeoutMs: number;
}

export interface McpServerStatus {
  name: string;
  transport: string;
  status: "connected" | "disconnected" | "connecting" | "error";
  toolCount: number;
  error?: string;
  connectedAt?: number;
}

interface ManagedServer {
  name: string;
  config: McpServerConfig;
  client: Client;
  transport: Transport;
  tools: string[];
  status: McpServerStatus["status"];
  error?: string;
  connectedAt?: number;
}

function resolveEnvVars(env: Record<string, string>): Record<string, string> {
  const result: Record<string, string> = {};
  for (const [key, value] of Object.entries(env)) {
    result[key] = value.replace(/\$\{(\w+)\}/g, (_, name) => process.env[name] ?? "");
  }
  return result;
}

export class McpClientManager {
  private servers = new Map<string, ManagedServer>();
  private registry: ToolRegistry;

  constructor(registry: ToolRegistry) {
    this.registry = registry;
  }

  async connectAll(servers: Record<string, McpServerConfig>): Promise<void> {
    const entries = Object.entries(servers);
    if (entries.length === 0) return;

    log.info(`Connecting to ${entries.length} MCP server(s)`);
    const results = await Promise.allSettled(
      entries.map(([name, config]) => this.connect(name, config)),
    );

    for (let i = 0; i < results.length; i++) {
      const result = results[i]!;
      const name = entries[i]![0];
      if (result.status === "rejected") {
        log.error(`Failed to connect to MCP server "${name}": ${result.reason}`);
      }
    }
  }

  async connect(name: string, config: McpServerConfig): Promise<void> {
    // Disconnect existing connection if any
    if (this.servers.has(name)) {
      await this.disconnect(name);
    }

    log.info(`Connecting to MCP server "${name}" (${config.transport})`);
    const transport = this.createTransport(name, config);

    const client = new Client(
      { name: "aletheia", version: getVersion() },
      { capabilities: {} },
    );

    const managed: ManagedServer = {
      name,
      config,
      client,
      transport,
      tools: [],
      status: "connecting",
    };
    this.servers.set(name, managed);

    try {
      await client.connect(transport);
      managed.status = "connected";
      managed.connectedAt = Date.now();
      log.info(`Connected to MCP server "${name}"`);

      // Discover and register tools
      await this.refreshTools(name);

      // Listen for tool list changes
      client.fallbackNotificationHandler = async (notification) => {
        if (notification.method === "notifications/tools/list_changed") {
          log.info(`Tools changed on MCP server "${name}", refreshing`);
          await this.refreshTools(name).catch((err) =>
            log.error(`Failed to refresh tools for "${name}": ${err}`),
          );
        }
      };
    } catch (err) {
      managed.status = "error";
      managed.error = err instanceof Error ? err.message : String(err);
      log.error(`MCP server "${name}" connection failed: ${managed.error}`);
      throw err;
    }
  }

  private createTransport(name: string, config: McpServerConfig): Transport {
    switch (config.transport) {
      case "stdio": {
        if (!config.command) {
          throw new ConfigError(`MCP server "${name}": stdio transport requires "command"`, {
            code: "CONFIG_MISSING_REQUIRED", context: { server: name, transport: "stdio" },
          });
        }
        const env = { ...process.env, ...resolveEnvVars(config.env) };
        return new StdioClientTransport({
          command: config.command,
          args: config.args,
          env: env as Record<string, string>,
          stderr: "pipe",
        });
      }
      case "sse": {
        if (!config.url) {
          throw new ConfigError(`MCP server "${name}": sse transport requires "url"`, {
            code: "CONFIG_MISSING_REQUIRED", context: { server: name, transport: "sse" },
          });
        }
        return new SSEClientTransport(new URL(config.url), {
          requestInit: {
            headers: resolveEnvVars(config.headers),
          },
        });
      }
      case "http": {
        if (!config.url) {
          throw new ConfigError(`MCP server "${name}": http transport requires "url"`, {
            code: "CONFIG_MISSING_REQUIRED", context: { server: name, transport: "http" },
          });
        }
        // Cast needed: StreamableHTTPClientTransport.sessionId is string|undefined
        // but Transport interface with exactOptionalPropertyTypes expects string
        return new StreamableHTTPClientTransport(new URL(config.url), {
          requestInit: {
            headers: resolveEnvVars(config.headers),
          },
        }) as unknown as Transport;
      }
      default:
        throw new ConfigError(`MCP server "${name}": unknown transport "${config.transport}"`, {
          code: "CONFIG_VALIDATION_FAILED", context: { server: name, transport: config.transport },
        });
    }
  }

  async refreshTools(name: string): Promise<void> {
    const managed = this.servers.get(name);
    if (!managed || managed.status !== "connected") return;

    // Unregister old tools from this server
    for (const toolName of managed.tools) {
      this.registry.unregister(toolName);
    }
    managed.tools = [];

    try {
      const result = await managed.client.listTools();
      const tools = result.tools ?? [];
      log.info(`MCP server "${name}" has ${tools.length} tool(s)`);

      for (const tool of tools) {
        const qualifiedName = `mcp__${name}__${tool.name}`;
        const handler: ToolHandler = {
          definition: {
            name: qualifiedName,
            description: `[MCP: ${name}] ${tool.description ?? tool.name}`,
            input_schema: (tool.inputSchema ?? { type: "object", properties: {} }) as Record<string, unknown>,
          },
          category: "available",
          execute: async (input: Record<string, unknown>, _context: ToolContext): Promise<string> => {
            const callResult = await managed.client.callTool({
              name: tool.name,
              arguments: input,
            });
            // Extract text from MCP content array
            const contents = callResult.content as Array<{ type: string; text?: string }>;
            if (!contents || contents.length === 0) return "";
            return contents
              .filter((c) => c.type === "text" && c.text)
              .map((c) => c.text!)
              .join("\n");
          },
        };

        this.registry.register(handler);
        managed.tools.push(qualifiedName);
      }
    } catch (err) {
      log.error(`Failed to list tools from MCP server "${name}": ${err instanceof Error ? err.message : err}`);
    }
  }

  async disconnect(name: string): Promise<void> {
    const managed = this.servers.get(name);
    if (!managed) return;

    // Unregister tools
    for (const toolName of managed.tools) {
      this.registry.unregister(toolName);
    }

    try {
      await managed.client.close();
    } catch (err) {
      log.warn(`Error closing MCP server "${name}": ${err instanceof Error ? err.message : err}`);
    }

    this.servers.delete(name);
    log.info(`Disconnected MCP server "${name}"`);
  }

  async disconnectAll(): Promise<void> {
    const names = [...this.servers.keys()];
    for (const name of names) {
      await this.disconnect(name);
    }
  }

  getStatus(): McpServerStatus[] {
    return [...this.servers.values()].map((s) => ({
      name: s.name,
      transport: s.config.transport,
      status: s.status,
      toolCount: s.tools.length,
      ...(s.error ? { error: s.error } : {}),
      ...(s.connectedAt ? { connectedAt: s.connectedAt } : {}),
    }));
  }

  getToolCount(): number {
    let count = 0;
    for (const server of this.servers.values()) {
      count += server.tools.length;
    }
    return count;
  }
}
