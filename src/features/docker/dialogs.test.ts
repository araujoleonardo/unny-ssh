// @vitest-environment jsdom
import { test, expect, vi } from "vitest";
import { downOptions } from "./dialogs";
test("down leaves destructive volume removal unchecked by default", async () => {
  document.body.innerHTML = '<div id="app"></div>';
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
    configurable: true,
    value: function (this: HTMLDialogElement) {
      this.open = true;
    },
  });
  Object.defineProperty(HTMLDialogElement.prototype, "close", {
    configurable: true,
    value: function (this: HTMLDialogElement) {
      this.open = false;
      this.dispatchEvent(new Event("close"));
    },
  });
  const result = downOptions("server:22", "fixture");
  expect(
    document.querySelector<HTMLInputElement>('[type="checkbox"]')!.checked,
  ).toBe(false);
  expect(document.querySelector("dialog")!.textContent).toContain("server:22");
  document.querySelector<HTMLButtonElement>(".danger")!.click();
  expect(await result).toEqual({ volumes: false });
  vi.restoreAllMocks();
});
