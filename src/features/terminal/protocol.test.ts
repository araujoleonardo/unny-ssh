import { expect, test } from "vitest";
import { TerminalOrder } from "./protocol";
import type { TerminalEvent } from "../../ipc/contracts";
const event: TerminalEvent = {
  terminalId: "terminal-a",
  sessionId: "session-a",
  generation: 3,
  sequence: 1,
  kind: "data",
  bytes: [0xc3],
  exitStatus: null,
};
test("terminal output stays bound to its session, generation, ID and sequence", () => {
  const order = new TerminalOrder("session-a", 3);
  expect(order.accept({ ...event, sessionId: "session-b" })).toBe(false);
  expect(order.accept({ ...event, generation: 2 })).toBe(false);
  expect(order.accept(event)).toBe(true);
  expect(order.accept(event)).toBe(false);
  expect(
    order.accept({ ...event, sequence: 2, terminalId: "terminal-b" }),
  ).toBe(false);
  expect(order.accept({ ...event, sequence: 2, bytes: [0xa9] })).toBe(true);
  expect(order.accept({ ...event, sequence: 3, kind: "closed" })).toBe(true);
  expect(order.accept({ ...event, sequence: 4 })).toBe(false);
});
test("cancellation permanently discards late output", () => {
  const order = new TerminalOrder("session-a", 3);
  order.cancel();
  expect(order.accept(event)).toBe(false);
});
