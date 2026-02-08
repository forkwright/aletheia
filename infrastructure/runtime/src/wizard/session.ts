// Stub: wizard session for headless gateway
import type { WizardPrompter } from "./prompts.js";

export type WizardSessionStatus = "idle" | "running" | "done" | "cancelled" | "error";

export type WizardStepResult = {
  done: boolean;
  step?: unknown;
  status?: WizardSessionStatus;
  error?: string;
};

type WizardRunner = (prompter: WizardPrompter) => Promise<void>;

export class WizardSession {
  #status: WizardSessionStatus = "idle";
  #error: string | undefined;
  #runner: WizardRunner;

  constructor(runner: WizardRunner) {
    this.#runner = runner;
  }

  getStatus(): WizardSessionStatus {
    return this.#status;
  }

  getError(): string | undefined {
    return this.#error;
  }

  async next(): Promise<WizardStepResult> {
    if (this.#status === "idle") {
      this.#status = "error";
      this.#error = "Interactive wizard not available in headless mode";
    }
    return {
      done: true,
      status: this.#status,
      error: this.#error,
    };
  }

  async answer(_stepId: string, _value: unknown): Promise<void> {
    throw new Error("Interactive wizard not available in headless mode");
  }

  cancel(): void {
    this.#status = "cancelled";
  }
}
