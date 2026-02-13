import type { OutboundSendDeps } from "../infra/outbound/deliver.js";
import { sendMessageSignal } from "../signal/send.js";

export type CliDeps = {
  sendMessageSignal: typeof sendMessageSignal;
};

export function createDefaultDeps(): CliDeps {
  return {
    sendMessageSignal,
  };
}

export function createOutboundSendDeps(deps: CliDeps): OutboundSendDeps {
  return {
    sendSignal: deps.sendMessageSignal,
  };
}
