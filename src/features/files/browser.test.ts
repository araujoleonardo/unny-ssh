// @vitest-environment jsdom
import { test, expect, vi, afterEach } from "vitest";
import { FileBrowser } from "./browser";
import type { RemoteEntry } from "../../ipc/files";
import type { SessionInfo } from "../../ipc/contracts";
vi.mock("../../ipc/files", () => ({
  listFiles: vi.fn(),
  mutateFiles: vi.fn(),
  releaseFiles: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("./dialogs", () => ({
  choose: vi.fn().mockResolvedValue("cancel"),
  askName: vi.fn(),
}));
import * as api from "../../ipc/files";
const entry = (
  id: string,
  kind: RemoteEntry["kind"] = "file",
): RemoteEntry => ({
  id,
  name: id,
  path: `/${id}`,
  kind,
  size: 0,
  permissions: 0,
  mtime: 0,
  escaped: false,
});
const session = { id: "s", state: "connected", sftp: true } as SessionInfo;
afterEach(() => vi.clearAllMocks());
test("loads only expanded folders and virtualizes large listings", async () => {
  const root = entry("root", "directory"),
    folder = entry("folder", "directory");
  vi.mocked(api.listFiles)
    .mockResolvedValueOnce({
      directory: root,
      parent: null,
      entries: [
        folder,
        ...Array.from({ length: 199 }, (_, i) => entry(`file${i}`)),
      ],
      nextCursor: "more",
    })
    .mockResolvedValueOnce({
      directory: folder,
      parent: root,
      entries: [entry("child")],
      nextCursor: null,
    });
  const host = document.createElement("div");
  document.body.append(host);
  const browser = new FileBrowser(1, host, vi.fn(), vi.fn(), vi.fn(), vi.fn());
  browser.show(session);
  await vi.waitFor(() =>
    expect(host.querySelector(".remote-file")).not.toBeNull(),
  );
  expect(api.listFiles).toHaveBeenCalledTimes(1);
  expect(host.querySelectorAll(".remote-file").length).toBeLessThan(30);
  const folderButton = [
    ...host.querySelectorAll<HTMLButtonElement>(".remote-file"),
  ].find((b) => b.textContent?.includes("folder"))!;
  folderButton.click();
  await vi.waitFor(() =>
    expect(api.listFiles).toHaveBeenCalledWith(1, "s", "folder", undefined),
  );
  expect(host.textContent).toContain("child");
  browser.dispose();
  expect(api.releaseFiles).toHaveBeenCalledWith(1, "s", undefined, "more");
  host.remove();
});
test("cancelled removal never applies a confirmation token", async () => {
  const root = entry("root", "directory");
  vi.mocked(api.listFiles).mockResolvedValue({
    directory: root,
    parent: null,
    entries: [],
    nextCursor: null,
  });
  vi.mocked(api.mutateFiles).mockResolvedValue({
    kind: "confirmation",
    token: "token",
    entry: root,
    target: null,
  });
  const host = document.createElement("div");
  document.body.append(host);
  const browser = new FileBrowser(1, host, vi.fn(), vi.fn(), vi.fn(), vi.fn());
  browser.show(session);
  await vi.waitFor(() =>
    expect(host.querySelector(".remote-file")).not.toBeNull(),
  );
  host
    .querySelector(".remote-file")!
    .dispatchEvent(
      new MouseEvent("contextmenu", { bubbles: true, cancelable: true }),
    );
  host.querySelector<HTMLButtonElement>('[data-action="delete"]')!.click();
  await vi.waitFor(() => expect(api.mutateFiles).toHaveBeenCalledTimes(1));
  expect(api.mutateFiles).toHaveBeenCalledWith(1, "s", {
    action: "prepare",
    entry: "root",
    name: null,
  });
  browser.dispose();
  host.remove();
});

test("folder context menu opens the selected directory in its own session and closes on Escape", async () => {
  const root = entry("root", "directory"),
    folder = entry("nested", "directory");
  vi.mocked(api.listFiles).mockResolvedValue({
    directory: root,
    parent: null,
    entries: [folder],
    nextCursor: null,
  });
  const host = document.createElement("div");
  document.body.append(host);
  const terminal = vi.fn();
  const browser = new FileBrowser(
    1,
    host,
    vi.fn(),
    vi.fn(),
    vi.fn(),
    vi.fn(),
    terminal,
  );
  browser.show(session);
  await vi.waitFor(() =>
    expect(host.querySelectorAll(".remote-file")).toHaveLength(2),
  );
  const row = host.querySelectorAll<HTMLElement>(".remote-file")[1];
  row.dispatchEvent(
    new MouseEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 80,
      clientY: 100,
    }),
  );
  host.querySelector<HTMLButtonElement>("[data-action=terminal]")!.click();
  expect(terminal).toHaveBeenCalledWith("s", folder);
  expect(host.querySelector<HTMLElement>("[role=menu]")!.hidden).toBe(true);
  row.dispatchEvent(
    new KeyboardEvent("keydown", {
      key: "F10",
      shiftKey: true,
      bubbles: true,
      cancelable: true,
    }),
  );
  const menu = host.querySelector<HTMLElement>("[role=menu]")!;
  expect(menu.hidden).toBe(false);
  menu.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
  );
  expect(menu.hidden).toBe(true);
  expect(api.mutateFiles).not.toHaveBeenCalled();
  browser.dispose();
  host.remove();
});

test("orders folders before files at every level and merges later pages in natural name order", async () => {
  const root = entry("root", "directory"),
    src = entry("src", "directory");
  vi.mocked(api.listFiles)
    .mockResolvedValueOnce({
      directory: root,
      parent: null,
      entries: [
        entry("file10.txt"),
        src,
        entry("File2.txt"),
        entry("z-link", "symlink"),
      ],
      nextCursor: "next",
    })
    .mockResolvedValueOnce({
      directory: src,
      parent: root,
      entries: [entry("z.txt"), entry("nested", "directory"), entry("A.txt")],
      nextCursor: null,
    })
    .mockResolvedValueOnce({
      directory: root,
      parent: null,
      entries: [
        entry("file1.txt"),
        entry("assets", "directory"),
        entry(".env"),
      ],
      nextCursor: null,
    });
  const host = document.createElement("div");
  document.body.append(host);
  const browser = new FileBrowser(1, host, vi.fn(), vi.fn(), vi.fn(), vi.fn());
  const rows = () => [
    ...host.querySelectorAll<HTMLButtonElement>(".remote-file"),
  ];
  const labels = () => rows().map((b) => b.textContent);
  browser.show(session);
  await vi.waitFor(() =>
    expect(labels()).toEqual([
      "root",
      "src",
      "File2.txt",
      "file10.txt",
      "z-link",
      "Carregar mais…",
    ]),
  );
  rows()
    .find((b) => b.textContent === "src")!
    .click();
  await vi.waitFor(() =>
    expect(labels()).toEqual([
      "root",
      "src",
      "nested",
      "A.txt",
      "z.txt",
      "File2.txt",
      "file10.txt",
      "z-link",
      "Carregar mais…",
    ]),
  );
  rows()
    .find((b) => b.textContent === "Carregar mais…")!
    .click();
  await vi.waitFor(() =>
    expect(labels()).toEqual([
      "root",
      "assets",
      "src",
      "nested",
      "A.txt",
      "z.txt",
      ".env",
      "file1.txt",
      "File2.txt",
      "file10.txt",
      "z-link",
    ]),
  );
  expect(api.listFiles).toHaveBeenLastCalledWith(1, "s", "root", "next");
  browser.dispose();
  host.remove();
});
