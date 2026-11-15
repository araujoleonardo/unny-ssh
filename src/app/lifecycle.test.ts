import { test, expect, vi } from "vitest";
import { IdleLock, LockCoordinator } from "./lifecycle";
test("only local activity resets idle time and a suspended clock locks on resume", () => {
  let now = 0;
  const lock = vi.fn();
  const timer = new IdleLock(
    () => now,
    () => 15,
    lock,
  );
  for (let i = 0; i < 600; i++) {
    now += 1000;
    timer.tick();
  }
  expect(lock).not.toHaveBeenCalled();
  timer.activity();
  for (let i = 0; i < 900; i++) {
    now += 1000;
    timer.tick();
  }
  expect(lock).toHaveBeenLastCalledWith("idle");
  timer.activity();
  now += 35_000;
  timer.tick();
  expect(lock).toHaveBeenLastCalledWith("resume");
});
test("lock hides content first and commits an acknowledged lock_failed state", async () => {
  const calls: string[] = [];
  const buffer = { sessionId: "s", documentId: "d", text: "unsaved" };
  const coordinator = new LockCoordinator({
    hide: () => {
      calls.push("hide");
    },
    restore: vi.fn(),
    persist: async (buffers) => {
      calls.push("persist");
      expect(buffers[0].text).toBe("unsaved");
      return { state: "lock_failed", generation: 2 };
    },
    finish: (status) => {
      calls.push(status.state);
    },
    failed: vi.fn(),
  });
  coordinator.register({
    capture: async () => {
      calls.push("capture");
      return [buffer];
    },
  });
  await coordinator.lock();
  expect(calls).toEqual(["hide", "capture", "persist", "lock_failed"]);
  expect(buffer.text).toBe("");
});
test("capture failure retains models and does not claim that locking succeeded", async () => {
  const finish = vi.fn(),
    restore = vi.fn(),
    failed = vi.fn();
  const coordinator = new LockCoordinator({
    hide: vi.fn(),
    restore,
    persist: vi.fn(),
    finish,
    failed,
  });
  coordinator.register({
    capture: async () => {
      throw new Error("capture");
    },
  });
  await coordinator.lock();
  expect(finish).not.toHaveBeenCalled();
  expect(restore).toHaveBeenCalledOnce();
  expect(failed).toHaveBeenCalledOnce();
});
