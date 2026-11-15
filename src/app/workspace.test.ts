// @vitest-environment jsdom
import { afterEach, expect, test, vi } from "vitest";
vi.mock("../ipc/connections", () => ({
  profilesImport: vi.fn().mockResolvedValue(null),
  profilesExport: vi.fn().mockResolvedValue(null),
  profilesList: vi.fn().mockResolvedValue([
    {
      id: "profile",
      name: "Fixture",
      host: "example.invalid",
      port: 22,
      username: "fixture",
      authMode: "agent",
      initialDirectory: null,
      hasSavedSecret: false,
    },
  ]),
  sessionsList: vi.fn().mockResolvedValue([]),
  sessionConnect: vi.fn().mockResolvedValue({
    id: "session",
    generation: 1,
    profile: {
      id: "profile",
      name: "Fixture",
      host: "example.invalid",
      port: 22,
      username: "fixture",
      authMode: "agent",
      initialDirectory: null,
    },
    state: "connecting",
    challenge: null,
    sftp: false,
  }),
}));
vi.mock("../features/terminal/live", () => ({
  TerminalPool: class {
    show() {}
    refresh() {}
    dispose() {}
  },
}));
vi.mock("../features/editor/live", () => ({
  EditorPool: class {
    show() {}
    dispose() {}
    async guard() {
      return true;
    }
  },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
import * as api from "../ipc/connections";
import { mountWorkspace } from "./workspace";
afterEach(() => {
  vi.clearAllMocks();
  vi.useRealTimers();
  localStorage.clear();
});
test("single selection preserves the target for double-click connection", async () => {
  vi.useFakeTimers();
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.querySelector<HTMLElement>("#app")!;
  const dispose = mountWorkspace(root, 1, vi.fn(), vi.fn());
  await vi.waitFor(() =>
    expect(root.querySelector(".profile-select")).not.toBeNull(),
  );
  const select = root.querySelector<HTMLButtonElement>(".profile-select")!;
  select.click();
  expect(select.isConnected).toBe(true);
  select.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
  await vi.waitFor(() =>
    expect(api.sessionConnect).toHaveBeenCalledWith(1, "profile", undefined),
  );
  dispose();
});

test("an empty workspace reveals saved connections even if the sidebar was previously hidden", async () => {
  vi.useFakeTimers();
  localStorage.setItem(
    "unny.layout.v1",
    JSON.stringify({ sidebarHidden: true }),
  );
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.querySelector<HTMLElement>("#app")!;
  const dispose = mountWorkspace(root, 1, vi.fn(), vi.fn());
  await vi.waitFor(() =>
    expect(root.querySelector(".profile-select")).not.toBeNull(),
  );
  expect(root.querySelector<HTMLElement>("#connections-panel")!.hidden).toBe(
    false,
  );
  expect(
    root.querySelector(".workspace")!.classList.contains("sidebar-hidden"),
  ).toBe(false);
  expect(root.querySelector<HTMLElement>("#files-panel")!.hidden).toBe(true);
  dispose();
});

test("import actions share a busy guard and refresh the profiles after success", async () => {
  vi.useFakeTimers();
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.querySelector<HTMLElement>("#app")!;
  let resolve!: (value: api.ProfileImportResult | null) => void;
  vi.mocked(api.profilesImport).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  const dispose = mountWorkspace(root, 1, vi.fn(), vi.fn());
  await vi.waitFor(() =>
    expect(root.querySelector(".profile-select")).not.toBeNull(),
  );
  root.querySelector<HTMLButtonElement>("#import-profiles")!.click();
  expect(
    root.querySelector<HTMLButtonElement>("#export-profiles")!.disabled,
  ).toBe(true);
  root.querySelector<HTMLButtonElement>("#connection-switcher")!.click();
  root.querySelector<HTMLButtonElement>("[data-import]")!.click();
  expect(api.profilesImport).toHaveBeenCalledTimes(1);
  expect(api.profilesImport).toHaveBeenCalledWith(1);
  resolve({ imported: 2, skipped: 1, foldersImported: 1 });
  await vi.waitFor(() =>
    expect(root.querySelector("#notice")!.textContent).toContain(
      "2 conexões importadas",
    ),
  );
  expect(root.querySelector("#notice")!.textContent).toContain(
    "1 já existentes ignoradas",
  );
  expect(api.profilesList).toHaveBeenCalledTimes(2);
  expect(
    root.querySelector<HTMLButtonElement>("#import-profiles")!.disabled,
  ).toBe(false);
  dispose();
});
test("cancelled file dialogs preserve the list and export reports only a count", async () => {
  vi.useFakeTimers();
  document.body.innerHTML = '<div id="app"></div>';
  const root = document.querySelector<HTMLElement>("#app")!;
  const dispose = mountWorkspace(root, 1, vi.fn(), vi.fn());
  await vi.waitFor(() =>
    expect(root.querySelector(".profile-select")).not.toBeNull(),
  );
  root.querySelector<HTMLButtonElement>("#import-profiles")!.click();
  await vi.waitFor(() =>
    expect(
      root.querySelector<HTMLButtonElement>("#import-profiles")!.disabled,
    ).toBe(false),
  );
  expect(api.profilesList).toHaveBeenCalledTimes(1);
  vi.mocked(api.profilesExport).mockResolvedValueOnce(1);
  root.querySelector<HTMLButtonElement>("#export-profiles")!.click();
  await vi.waitFor(() =>
    expect(root.querySelector("#notice")!.textContent).toContain(
      "1 conexões exportadas sem senhas",
    ),
  );
  expect(api.profilesExport).toHaveBeenCalledWith(1);
  dispose();
});
