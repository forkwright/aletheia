// Structured error hierarchy for machine-readable error handling
import type { ErrorCode } from "./error-codes.js";

export interface AletheiaErrorOpts {
  code: ErrorCode;
  module: string;
  message: string;
  context?: Record<string, unknown> | undefined;
  recoverable?: boolean | undefined;
  retryAfterMs?: number | undefined;
  cause?: unknown;
}

export class AletheiaError extends Error {
  readonly code: ErrorCode;
  readonly module: string;
  readonly context: Record<string, unknown>;
  readonly recoverable: boolean;
  readonly retryAfterMs?: number | undefined;
  readonly timestamp: string;

  constructor(opts: AletheiaErrorOpts) {
    super(opts.message, { cause: opts.cause });
    this.name = "AletheiaError";
    this.code = opts.code;
    this.module = opts.module;
    this.context = opts.context ?? {};
    this.recoverable = opts.recoverable ?? false;
    this.retryAfterMs = opts.retryAfterMs;
    this.timestamp = new Date().toISOString();
  }

  toJSON(): Record<string, unknown> {
    return {
      error: this.code,
      module: this.module,
      message: this.message,
      context: this.context,
      recoverable: this.recoverable,
      retryAfterMs: this.retryAfterMs,
      timestamp: this.timestamp,
      stack: this.stack,
    };
  }
}

export class ConfigError extends AletheiaError {
  constructor(message: string, opts?: { cause?: unknown; context?: Record<string, unknown>; code?: ErrorCode }) {
    super({
      code: opts?.code ?? "CONFIG_VALIDATION_FAILED",
      module: "taxis",
      message,
      context: opts?.context,
      cause: opts?.cause,
    });
    this.name = "ConfigError";
  }
}

export class SessionError extends AletheiaError {
  constructor(message: string, opts?: { cause?: unknown; context?: Record<string, unknown>; code?: ErrorCode }) {
    super({
      code: opts?.code ?? "SESSION_NOT_FOUND",
      module: "mneme",
      message,
      context: opts?.context,
      cause: opts?.cause,
    });
    this.name = "SessionError";
  }
}

export class ProviderError extends AletheiaError {
  constructor(message: string, opts?: { cause?: unknown; context?: Record<string, unknown>; code?: ErrorCode; recoverable?: boolean; retryAfterMs?: number }) {
    super({
      code: opts?.code ?? "PROVIDER_TIMEOUT",
      module: "hermeneus",
      message,
      context: opts?.context,
      recoverable: opts?.recoverable,
      retryAfterMs: opts?.retryAfterMs,
      cause: opts?.cause,
    });
    this.name = "ProviderError";
  }
}

export class ToolError extends AletheiaError {
  constructor(message: string, opts?: { cause?: unknown; context?: Record<string, unknown>; code?: ErrorCode }) {
    super({
      code: opts?.code ?? "TOOL_EXECUTION_FAILED",
      module: "organon",
      message,
      context: opts?.context,
      cause: opts?.cause,
    });
    this.name = "ToolError";
  }
}

export class PipelineError extends AletheiaError {
  constructor(message: string, opts?: { cause?: unknown; context?: Record<string, unknown>; code?: ErrorCode; recoverable?: boolean }) {
    super({
      code: opts?.code ?? "PIPELINE_STAGE_FAILED",
      module: "nous",
      message,
      context: opts?.context,
      recoverable: opts?.recoverable,
      cause: opts?.cause,
    });
    this.name = "PipelineError";
  }
}

export class StoreError extends AletheiaError {
  constructor(message: string, opts?: { cause?: unknown; context?: Record<string, unknown>; code?: ErrorCode }) {
    super({
      code: opts?.code ?? "STORE_INIT_FAILED",
      module: "mneme",
      message,
      context: opts?.context,
      cause: opts?.cause,
    });
    this.name = "StoreError";
  }
}

export class TransportError extends AletheiaError {
  constructor(message: string, opts?: { cause?: unknown; context?: Record<string, unknown>; code?: ErrorCode; recoverable?: boolean; retryAfterMs?: number }) {
    super({
      code: opts?.code ?? "SIGNAL_SEND_FAILED",
      module: "semeion",
      message,
      context: opts?.context,
      recoverable: opts?.recoverable,
      retryAfterMs: opts?.retryAfterMs,
      cause: opts?.cause,
    });
    this.name = "TransportError";
  }
}
