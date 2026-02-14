#!/usr/bin/env node
// CLI entry point
import { Command } from "commander";
import { startRuntime } from "./aletheia.js";

const program = new Command()
  .name("aletheia")
  .description("Aletheia distributed cognition runtime")
  .version("0.1.0");

program
  .command("gateway")
  .description("Gateway management")
  .command("start")
  .description("Start the gateway")
  .option("-c, --config <path>", "Config file path")
  .action(async (opts: { config?: string }) => {
    await startRuntime(opts.config);
  });

program
  .command("doctor")
  .description("Validate configuration")
  .option("-c, --config <path>", "Config file path")
  .action((opts: { config?: string }) => {
    const { loadConfig } = require("./taxis/loader.js");
    try {
      const config = loadConfig(opts.config);
      console.log("Config valid.");
      console.log(`  Nous: ${config.agents.list.map((a: { id: string }) => a.id).join(", ")}`);
      console.log(`  Bindings: ${config.bindings.length}`);
      console.log(`  Gateway port: ${config.gateway.port}`);
      console.log(`  Plugins: ${Object.keys(config.plugins.entries).length}`);
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
