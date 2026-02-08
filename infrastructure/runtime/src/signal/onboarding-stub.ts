// Stub: full onboarding adapter removed in headless-only strip
import type { ChannelOnboardingAdapter } from "../channels/plugins/onboarding-types.js";

export const signalOnboardingAdapter: ChannelOnboardingAdapter = {
  channel: "signal",
  getStatus: async () => ({
    channel: "signal",
    configured: false,
    statusLines: ["Signal onboarding: headless mode (use config directly)"],
  }),
  configure: async ({ cfg }) => ({ cfg }),
};
