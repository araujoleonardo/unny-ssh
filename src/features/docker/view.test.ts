// @vitest-environment jsdom
import { afterEach, beforeEach, expect, test, vi } from "vitest";
vi.mock("../../ipc/docker", () => ({
  inventory: vi.fn(),
  projects: vi.fn(),
  prepare: vi.fn(),
  apply: vi.fn(),
  streamAction: vi.fn().mockResolvedValue(undefined),
  logs: vi.fn(),
  register: vi.fn(),
  removeProject: vi.fn(),
  projectStatus: vi.fn(),
  prepareCompose: vi.fn(),
  applyCompose: vi.fn(),
}));
vi.mock("../files/dialogs", () => ({
  choose: vi.fn().mockResolvedValue("apply"),
}));
vi.mock("./dialogs", () => ({
  inputDialog: vi.fn(),
  downOptions: vi.fn(),
  inspection: vi.fn(),
}));
import * as api from "../../ipc/docker";
import { inputDialog, downOptions } from "./dialogs";
import { DockerPanel, allowed } from "./view";
import type { SessionInfo } from "../../ipc/contracts";
const resource = (kind: api.Kind, name: string): api.Resource => ({
  handle: name,
  kind,
  id: "a".repeat(64),
  name,
  state: "running",
  image: "fixture",
  ports: "",
  tags: "",
  project: "",
  details: "",
});
const session = (id: string) =>
  ({
    id,
    state: "connected",
    profile: {
      id: "profile",
      name: id,
      username: "fixture",
      host: "fixture.invalid",
      port: 22,
    },
  }) as SessionInfo;
let host: HTMLElement, panel: DockerPanel;
beforeEach(() => {
  vi.clearAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  panel = new DockerPanel(1, host, vi.fn(), vi.fn());
  vi.mocked(api.inventory).mockResolvedValue({
    version: "fixture",
    resources: [],
  });
  vi.mocked(api.projects).mockResolvedValue([]);
});
afterEach(() => {
  panel.dispose();
  host.remove();
});
test("inventory caches stay attached to their category", async () => {
  vi.mocked(api.inventory).mockImplementation(async (_g, _s, kind) => ({
    version: "fixture",
    resources: [
      resource(kind, kind === "container" ? "container-a" : "volume-b"),
    ],
  }));
  panel.show(session("a"), true);
  await vi.waitFor(() => expect(host.textContent).toContain("container-a"));
  host.querySelector<HTMLButtonElement>('[data-kind="volume"]')!.click();
  await vi.waitFor(() => expect(host.textContent).toContain("volume-b"));
  host.querySelector<HTMLButtonElement>('[data-kind="container"]')!.click();
  expect(host.querySelector(".docker-resources")!.textContent).toContain(
    "container-a",
  );
  expect(host.querySelector(".docker-resources")!.textContent).not.toContain(
    "volume-b",
  );
});
test("volume removal requires exact name; cancelling never dispatches the action", async () => {
  vi.mocked(api.inventory).mockImplementation(async (_g, _s, kind) => ({
    version: "fixture",
    resources: kind === "volume" ? [resource(kind, "important-data")] : [],
  }));
  vi.mocked(api.prepare).mockResolvedValue({
    token: "scoped-token",
    server: "fixture@fixture.invalid:22",
    name: "important-data",
    action: "rm",
    needsName: true,
  });
  vi.mocked(inputDialog).mockResolvedValue(null);
  panel.show(session("a"), true);
  await vi.waitFor(() =>
    expect(
      host.querySelector<HTMLButtonElement>('[data-kind="volume"]')!.disabled,
    ).toBe(false),
  );
  host.querySelector<HTMLButtonElement>('[data-kind="volume"]')!.click();
  await vi.waitFor(() => expect(host.textContent).toContain("important-data"));
  const remove = [
    ...host.querySelectorAll<HTMLButtonElement>(".docker-actions button"),
  ].find((b) => b.textContent === "Remover")!;
  remove.click();
  await vi.waitFor(() => expect(inputDialog).toHaveBeenCalledOnce());
  expect(api.apply).not.toHaveBeenCalled();
  expect(vi.mocked(inputDialog).mock.calls[0][1]).toContain(
    "fixture@fixture.invalid:22",
  );
  expect(vi.mocked(inputDialog).mock.calls[0][3]).toBe("important-data");
});
test("Compose volumes need an additional explicit confirmation", async () => {
  vi.mocked(api.projects).mockResolvedValue([
    {
      id: "p",
      name: "fixture",
      path: "/srv/fixture",
      identity: {
        profileId: "profile",
        host: "fixture.invalid",
        port: 22,
        username: "fixture",
      },
    },
  ]);
  vi.mocked(downOptions).mockResolvedValue({ volumes: true });
  vi.mocked(api.prepareCompose).mockResolvedValue({
    token: "down-token",
    server: "fixture.invalid:22",
    project: "fixture",
    path: "/srv/fixture",
    action: "down",
    volumeToken: "volume-token",
  });
  vi.mocked(inputDialog)
    .mockResolvedValueOnce("fixture")
    .mockResolvedValueOnce(null);
  panel.show(session("a"), true);
  await vi.waitFor(() =>
    expect(
      host.querySelector<HTMLButtonElement>('[data-kind="compose"]')!.disabled,
    ).toBe(false),
  );
  host.querySelector<HTMLButtonElement>('[data-kind="compose"]')!.click();
  await vi.waitFor(() =>
    expect(host.querySelector(".docker-resources")!.textContent).toContain(
      "/srv/fixture",
    ),
  );
  [...host.querySelectorAll<HTMLButtonElement>(".docker-actions button")]
    .find((b) => b.textContent === "Down")!
    .click();
  await vi.waitFor(() => expect(inputDialog).toHaveBeenCalledTimes(2));
  expect(api.prepareCompose).toHaveBeenCalledWith(1, "a", "p", "down", true);
  expect(vi.mocked(inputDialog).mock.calls[1][3]).toBe(
    "REMOVER VOLUMES fixture",
  );
  expect(api.applyCompose).not.toHaveBeenCalled();
});
test("late inventory never replaces the active server", async () => {
  let complete!: (v: { version: string; resources: api.Resource[] }) => void;
  vi.mocked(api.inventory).mockImplementation((_g, s) =>
    s === "a"
      ? new Promise((resolve) => {
          complete = resolve;
        })
      : Promise.resolve({
          version: "fixture",
          resources: [resource("container", "server-b")],
        }),
  );
  panel.show(session("a"), true);
  panel.show(session("b"), true);
  await vi.waitFor(() => expect(host.textContent).toContain("server-b"));
  complete({
    version: "fixture",
    resources: [resource("container", "server-a")],
  });
  await Promise.resolve();
  expect(host.querySelector(".docker-resources")!.textContent).toContain(
    "server-b",
  );
  expect(host.querySelector(".docker-resources")!.textContent).not.toContain(
    "server-a",
  );
});
test("action availability reflects server state", () => {
  expect(allowed(resource("container", "a"), "pause")).toBe(true);
  expect(allowed(resource("container", "a"), "remove")).toBe(false);
  expect(allowed(resource("volume", "v"), "start")).toBe(false);
});
