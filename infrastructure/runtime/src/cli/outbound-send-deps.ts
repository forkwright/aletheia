import type { OutboundSendDeps } from "../infra/outbound/deliver.js";

export type CliDeps = {
  sendMessageSignal: NonNullable<OutboundSendDeps["sendSignal"]>;
};

export function createOutboundSendDeps(deps: CliDeps): OutboundSendDeps {
  return {
    sendSignal: deps.sendMessageSignal,
  };
}
