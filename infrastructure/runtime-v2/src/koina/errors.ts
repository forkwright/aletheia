// Domain error types

export class AletheiaError extends Error {
  constructor(
    message: string,
    public readonly code: string,
    public readonly cause?: unknown,
  ) {
    super(message);
    this.name = "AletheiaError";
  }
}

export class ConfigError extends AletheiaError {
  constructor(message: string, cause?: unknown) {
    super(message, "CONFIG_ERROR", cause);
    this.name = "ConfigError";
  }
}

export class SessionError extends AletheiaError {
  constructor(message: string, cause?: unknown) {
    super(message, "SESSION_ERROR", cause);
    this.name = "SessionError";
  }
}

export class ProviderError extends AletheiaError {
  constructor(message: string, cause?: unknown) {
    super(message, "PROVIDER_ERROR", cause);
    this.name = "ProviderError";
  }
}

export class ToolError extends AletheiaError {
  constructor(message: string, cause?: unknown) {
    super(message, "TOOL_ERROR", cause);
    this.name = "ToolError";
  }
}
