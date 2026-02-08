import type { OnboardOptions } from "../commands/onboard-types.js";
// Stub: wizard UI removed for headless gateway builds
import type { RuntimeEnv } from "../runtime.js";
import type { WizardPrompter } from "./prompts.js";

export async function runOnboardingWizard(
  _opts: OnboardOptions,
  runtime: RuntimeEnv,
  _prompter: WizardPrompter,
): Promise<void> {
  runtime.error("Interactive onboarding wizard not available in headless mode");
  runtime.exit(1);
}
