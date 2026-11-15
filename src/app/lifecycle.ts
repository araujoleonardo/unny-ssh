import type { DirtyBuffer } from "../ipc/lifecycle";
import type { VaultStatus } from "../ipc/contracts";
export interface LockParticipant {
  capture(): Promise<DirtyBuffer[]>;
}
export class LockCoordinator {
  private participants = new Set<LockParticipant>();
  private busy = false;
  constructor(
    private effects: {
      hide(): void;
      restore(): void;
      persist(buffers: DirtyBuffer[]): Promise<VaultStatus>;
      finish(status: VaultStatus): void;
      failed(error: unknown): void;
    },
  ) {}
  register(participant: LockParticipant) {
    this.participants.add(participant);
    return () => this.participants.delete(participant);
  }
  async lock() {
    if (this.busy) return;
    this.busy = true;
    let buffers: DirtyBuffer[] = [];
    this.effects.hide();
    try {
      for (const p of this.participants) buffers.push(...(await p.capture()));
      const status = await this.effects.persist(buffers);
      this.effects.finish(status);
    } catch (error) {
      this.effects.restore();
      this.effects.failed(error);
    } finally {
      for (const b of buffers) b.text = "";
      buffers = [];
      this.busy = false;
    }
  }
}
export class IdleLock {
  private lastActivity: number;
  private lastTick: number;
  constructor(
    private now: () => number,
    private minutes: () => number,
    private lock: (reason: "idle" | "resume") => void,
  ) {
    this.lastActivity = this.lastTick = now();
  }
  activity() {
    this.lastActivity = this.now();
  }
  reset() {
    this.lastActivity = this.lastTick = this.now();
  }
  tick() {
    const now = this.now(),
      gap = now - this.lastTick;
    this.lastTick = now;
    if (gap > 30_000 || gap < 0) {
      this.lock("resume");
      this.lastActivity = now;
    } else if (now - this.lastActivity >= this.minutes() * 60_000) {
      this.lock("idle");
      this.lastActivity = now;
    }
  }
}
export function lockMinutes() {
  const n = Number(localStorage.getItem("unny.lock-minutes") ?? 15);
  return Number.isInteger(n) && n >= 1 && n <= 240 ? n : 15;
}
