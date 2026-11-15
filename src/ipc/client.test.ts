import { expect, test } from "vitest";
import { StreamConsumer } from "./client";
import type { StreamEvent } from "./contracts";
const event: StreamEvent = {
  operationId: "test",
  generation: 2,
  sequence: 1,
  state: "running",
  data: "",
};
test("ignores stale, duplicate, skipped and cancelled events", () => {
  const consumer = new StreamConsumer(2);
  expect(consumer.accept({ ...event, generation: 1 })).toBe(false);
  expect(consumer.accept({ ...event, sequence: 2 })).toBe(false);
  expect(consumer.accept(event)).toBe(true);
  expect(consumer.accept(event)).toBe(false);
  expect(consumer.accept({ ...event, operationId: "other", sequence: 2 })).toBe(
    false,
  );
  consumer.cancel();
  expect(consumer.accept({ ...event, sequence: 2 })).toBe(false);
});
