import { invoke, isTauri, Channel } from "@tauri-apps/api/core";
import type {
  AppError,
  StreamEvent,
  VaultRequest,
  VaultStatus,
} from "./contracts";
export const native = isTauri();
export const vaultStatus = () => invoke<VaultStatus>("vault_status");
export const vaultAction = (request: VaultRequest) =>
  invoke<VaultStatus>("vault_action", { request });
export function safeError(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    "code" in error
  ) {
    const e = error as AppError;
    return `${e.message} (${e.code} · ${e.correlationId})`;
  }
  return "Não foi possível concluir. Tente novamente no aplicativo desktop.";
}
export class StreamConsumer {
  private sequence = 0;
  private operationId: string | undefined;
  private closed = false;
  constructor(readonly generation: number) {}
  accept(event: StreamEvent): boolean {
    if (
      this.closed ||
      event.generation !== this.generation ||
      (this.operationId !== undefined &&
        event.operationId !== this.operationId) ||
      event.sequence !== this.sequence + 1
    )
      return false;
    this.operationId = event.operationId;
    this.sequence = event.sequence;
    if (event.state !== "running") this.closed = true;
    return true;
  }
  cancel() {
    this.closed = true;
  }
}
export async function openStream(
  generation: number,
  onEvent: (event: StreamEvent) => void,
) {
  const consumer = new StreamConsumer(generation);
  const channel = new Channel<StreamEvent>();
  channel.onmessage = (event) => {
    if (consumer.accept(event)) {
      onEvent(event);
      void invoke("stream_ack", {
        operationId: event.operationId,
        generation,
        sequence: event.sequence,
      }).catch(() => consumer.cancel());
    }
  };
  const operationId = await invoke<string>("stream_open", {
    generation,
    channel,
  });
  return async () => {
    consumer.cancel();
    await invoke("stream_cancel", { operationId, generation });
  };
}

export const resetChallenge = () => invoke<string>("reset_challenge");
