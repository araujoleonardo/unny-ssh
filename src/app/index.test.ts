// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
vi.mock("../ipc/client", () => ({ native: false }));
vi.mock("../features/editor/preview", () => ({
  mountEditor: vi.fn(() => vi.fn()),
}));
vi.mock("../features/terminal/preview", () => ({
  mountTerminal: vi.fn(() => vi.fn()),
}));
document.body.innerHTML = '<div id="app"></div>';
const { start } = await import("./index");
afterEach(() => {
  vi.useRealTimers();
  localStorage.clear();
});
test("session selection updates every panel and tools remain navigable", async () => {
  vi.useFakeTimers();
  await start();
  const select = document.querySelector<HTMLButtonElement>(
    '.session[data-session="1"]',
  )!;
  select.click();
  expect(document.querySelector(".breadcrumb")?.textContent).toContain(
    "Staging",
  );
  expect(document.querySelector(".files-heading")?.textContent).toContain(
    "Staging",
  );
  expect(document.querySelector(".terminal-island h2")?.textContent).toContain(
    "Staging",
  );
  expect(document.querySelector(".statusbar")?.textContent).toContain(
    "Staging",
  );
  document.querySelector<HTMLButtonElement>("#docker-tool")!.click();
  expect(document.querySelector<HTMLElement>("#docker")!.hidden).toBe(false);
  document.querySelector<HTMLButtonElement>("#connections-tool")!.click();
  expect(
    JSON.parse(localStorage.getItem("unny.layout.v1")!).sidebarHidden,
  ).toBe(true);
  document.querySelector<HTMLButtonElement>("#restore")!.click();
  expect(
    document.querySelector(".workspace")!.classList.contains("sidebar-hidden"),
  ).toBe(false);
});
