#!/usr/bin/env node
// CLI entry point
import { Command } from "commander";
import { startRuntime } from "./aletheia.js";
import { loadConfig } from "./taxis/loader.js";

const program = new Command()
  .name("aletheia")
  .description("Aletheia distributed cognition runtime")
  .version("0.1.0");

const gateway = program
  .command("gateway")
  .description("Gateway management");

gateway
  .command("start")
  .description("Start the gateway")
  .option("-c, --config <path>", "Config file path")
  .action(async (opts: { config?: string }) => {
    await startRuntime(opts.config);
  });

// Alias: "gateway run" → same as "gateway start" (systemd compat)
gateway
  .command("run")
  .description("Start the gateway (alias for start)")
  .option("-c, --config <path>", "Config file path")
  .action(async (opts: { config?: string }) => {
    await startRuntime(opts.config);
  });

program
  .command("doctor")
  .description("Validate configuration")
  .option("-c, --config <path>", "Config file path")
  .action((opts: { config?: string }) => {
    try {
      const config = loadConfig(opts.config);
      console.log("Config valid.");
      console.log(`  Nous: ${config.agents.list.map((a) => a.id).join(", ")}`);
      console.log(`  Bindings: ${config.bindings.length}`);
      console.log(`  Gateway port: ${config.gateway.port}`);
      console.log(`  Signal accounts: ${Object.keys(config.channels.signal.accounts).length}`);
      console.log(`  Plugins: ${Object.keys(config.plugins.entries).length}`);
      console.log(`  Plugin paths: ${config.plugins.load.paths.length}`);
    } catch (error) {
      console.error(
        "Config invalid:",
        error instanceof Error ? error.message : error,
      );
      process.exit(1);
    }
  });

program
  .command("status")
  .description("System health check")
  .action(() => {
    console.log("Status: not yet implemented");
  });

program.parse();
