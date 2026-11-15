// @vitest-environment jsdom
import { beforeEach, expect, test } from "vitest";
import { bindDivider, defaults, readLayout, saveLayout } from "./layout";
beforeEach(() => localStorage.clear());
test("rejects malformed preferences and constrains dimensions", () => {
  localStorage.setItem("unny.layout.v1", "broken");
  expect(readLayout()).toEqual(defaults);
  localStorage.setItem(
    "unny.layout.v1",
    JSON.stringify({ sidebar: 9999, terminal: -1, sidebarHidden: "true" }),
  );
  expect(readLayout()).toEqual({ ...defaults, sidebar: 420, terminal: 140 });
});
test("persists collapse and restoration", () => {
  saveLayout({ ...defaults, terminalHidden: true });
  expect(readLayout().terminalHidden).toBe(true);
  saveLayout(defaults);
  expect(readLayout()).toEqual(defaults);
});
test("keyboard divider updates accessible size", () => {
  const el = document.createElement("div");
  let value = 260;
  bindDivider(
    el,
    "x",
    () => value,
    (n) => (value = n),
  );
  el.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight" }));
  expect(value).toBe(280);
  expect(el.getAttribute("aria-valuenow")).toBe("280");
});
