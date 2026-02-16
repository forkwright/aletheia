// Structured logging for all modules
import { Logger } from "tslog";

export const log = new Logger({
  name: "aletheia",
  prettyLogTemplate:
    "{{dateIsoStr}} {{logLevelName}} [{{name}}] ",
  prettyErrorTemplate:
    "{{dateIsoStr}} {{logLevelName}} [{{name}}] {{errorName}} {{errorMessage}}\n{{errorStack}}",
  type: "pretty",
  minLevel: process.env["ALETHEIA_LOG_LEVEL"] === "debug" ? 0 : 3,
});

export function createLogger(name: string): Logger<unknown> {
  return log.getSubLogger({ name });
}
