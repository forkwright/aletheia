// Stub: wizard UI removed for headless gateway builds
import type { WizardPrompter } from "./prompts.js";

const HEADLESS_MSG = "Interactive wizard not available in headless mode";

export function createClackPrompter(): WizardPrompter {
  return {
    text: async () => {
      throw new Error(HEADLESS_MSG);
    },
    select: async () => {
      throw new Error(HEADLESS_MSG);
    },
    confirm: async () => {
      throw new Error(HEADLESS_MSG);
    },
    multiselect: async () => {
      throw new Error(HEADLESS_MSG);
    },
    note: () => {},
    intro: () => {},
    outro: () => {},
    spinner: () => ({
      start: () => {},
      stop: () => {},
    }),
    progress: () => ({
      update: () => {},
      stop: () => {},
    }),
  };
}
