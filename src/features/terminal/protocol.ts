import type { TerminalEvent } from "../../ipc/contracts";
export class TerminalOrder {
  private sequence = 0;
  private terminalId: string | undefined;
  private closed = false;
  constructor(
    readonly sessionId: string,
    readonly generation: number,
  ) {}
  accept(event: TerminalEvent) {
    if (
      this.closed ||
      event.sessionId !== this.sessionId ||
      event.generation !== this.generation ||
      event.sequence !== this.sequence + 1 ||
      (this.terminalId !== undefined && event.terminalId !== this.terminalId)
    )
      return false;
    this.terminalId = event.terminalId;
    this.sequence = event.sequence;
    if (event.kind !== "data") this.closed = true;
    return true;
  }
  cancel() {
    this.closed = true;
  }
}
