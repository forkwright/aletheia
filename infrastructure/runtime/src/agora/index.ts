// Agora — channel abstraction layer (Spec 34)
export { AgoraRegistry } from "./registry.js";
export { channelList, channelAddSlack, channelRemove, isSupportedChannel, listSupportedChannels } from "./cli.js";
export { SlackChannelProvider } from "./channels/slack/provider.js";
export type {
  ChannelCapabilities,
  ChannelContext,
  ChannelIdentity,
  ChannelProbeResult,
  ChannelProvider,
  ChannelSendParams,
  ChannelSendResult,
} from "./types.js";
