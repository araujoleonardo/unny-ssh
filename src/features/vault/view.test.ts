// @vitest-environment jsdom
import { expect, test, vi } from "vitest";
vi.mock("../../ipc/client", () => ({
  vaultAction: vi.fn().mockResolvedValue({ state: "unlocked", generation: 1 }),
  safeError: () => "Falha",
}));
import { vaultAction } from "../../ipc/client";
import { vaultForm } from "./view";
test("validates confirmation then clears password fields before IPC", async () => {
  const container = document.createElement("div");
  const done = vi.fn();
  vaultForm(container, "create", done);
  const inputs = container.querySelectorAll("input");
  inputs[0].value = "a long master password";
  inputs[1].value = "mismatch";
  const form = container.querySelector("form")!;
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(vaultAction).not.toHaveBeenCalled();
  inputs[1].value = inputs[0].value;
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(inputs[0].value).toBe("");
  expect(inputs[1].value).toBe("");
  await vi.waitFor(() =>
    expect(done).toHaveBeenCalledWith({ state: "unlocked", generation: 1 }),
  );
});
