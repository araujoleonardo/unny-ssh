// @vitest-environment jsdom
import { draftRestore } from "../../ipc/lifecycle";
import { test, expect, vi, beforeEach } from "vitest";
const fake = vi.hoisted(() => ({
  models: [] as any[],
  commands: [] as (() => void)[],
}));
vi.mock("../../ipc/lifecycle", () => ({ draftRestore: vi.fn() }));
vi.mock("./preview", () => ({}));
vi.mock(
  "monaco-editor/editor/contrib/find/browser/findController.js",
  () => ({}),
);
vi.mock("monaco-editor/languages/definitions/yaml/register.js", () => ({}));
vi.mock(
  "monaco-editor/languages/definitions/javascript/register.js",
  () => ({}),
);
vi.mock("monaco-editor/editor/editor.api.js", () => ({
  KeyMod: { CtrlCmd: 1 },
  KeyCode: { KeyS: 1 },
  editor: {
    create: () => ({
      setModel: vi.fn(),
      focus: vi.fn(),
      dispose: vi.fn(),
      addCommand: (_key: number, fn: () => void) => fake.commands.push(fn),
    }),
    createModel: (text: string) => {
      let version = 1;
      let changed = () => {};
      const model = {
        getValue: () => text,
        getAlternativeVersionId: () => version,
        onDidChangeContent: (fn: () => void) => {
          changed = fn;
          return { dispose: vi.fn() };
        },
        setValue: (next: string) => {
          text = next;
          version++;
          changed();
        },
        dispose: vi.fn(),
      };
      fake.models.push(model);
      return model;
    },
  },
}));
vi.mock("../../ipc/files", () => ({
  openDocument: vi.fn(),
  saveDocument: vi.fn(),
  releaseFiles: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../files/dialogs", () => ({ choose: vi.fn() }));
import { EditorPool } from "./live";
import * as api from "../../ipc/files";
import { choose } from "../files/dialogs";
const entry = {
  id: "file",
  name: "hello.txt",
  path: "/hello.txt",
  kind: "file" as const,
  size: 3,
  mtime: 0,
  permissions: 0o640,
  escaped: false,
};
const revision = { size: 3, mtime: 0, sha256: "original" };
beforeEach(() => {
  fake.models = [];
  fake.commands = [];
  vi.clearAllMocks();
  vi.mocked(api.openDocument).mockResolvedValue({
    kind: "document",
    document: {
      id: "doc",
      entry,
      text: "old",
      revision,
      bom: false,
      lineEndings: "LF",
    },
  });
});
test("save failures retain dirty buffers and cancel prevents closing", async () => {
  const strip = document.createElement("div");
  const pool = new EditorPool(
    1,
    strip,
    document.createElement("div"),
    vi.fn(),
    vi.fn(),
    vi.fn(),
  );
  pool.show("s");
  await pool.open("s", entry);
  fake.models[0].setValue("unsaved");
  expect(strip.textContent).toContain("●");
  vi.mocked(api.saveDocument).mockRejectedValue({
    code: "CONNECTION_LOST",
    message: "offline",
  });
  expect(await pool.save()).toBe(false);
  expect(fake.models[0].getValue()).toBe("unsaved");
  vi.mocked(choose).mockResolvedValue("cancel");
  expect(await pool.guard()).toBe(false);
  pool.dispose();
  expect(fake.models[0].dispose).toHaveBeenCalledOnce();
});
test("conflict confirmation binds overwrite to the returned revision token", async () => {
  const pool = new EditorPool(
    1,
    document.createElement("div"),
    document.createElement("div"),
    vi.fn(),
    vi.fn(),
    vi.fn(),
  );
  pool.show("s");
  await pool.open("s", entry);
  fake.models[0].setValue("new");
  vi.mocked(api.saveDocument)
    .mockResolvedValueOnce({
      kind: "conflict",
      token: "bound-token",
      current: revision,
      missing: false,
    })
    .mockResolvedValueOnce({
      kind: "saved",
      revision: { ...revision, sha256: "new" },
    });
  vi.mocked(choose).mockResolvedValue("overwrite");
  expect(await pool.save()).toBe(true);
  expect(api.saveDocument).toHaveBeenLastCalledWith(
    1,
    "s",
    "doc",
    revision,
    "new",
    "bound-token",
  );
  expect(await pool.guard()).toBe(true);
  pool.dispose();
});

test("a delayed file open never replaces the active session editor", async () => {
  const reveal = vi.fn();
  const strip = document.createElement("div");
  const pool = new EditorPool(
    1,
    strip,
    document.createElement("div"),
    vi.fn(),
    reveal,
    vi.fn(),
  );
  let complete!: (value: api.OpenReply) => void;
  vi.mocked(api.openDocument).mockReturnValueOnce(
    new Promise((resolve) => {
      complete = resolve;
    }),
  );
  pool.show("a");
  const opening = pool.open("a", entry);
  pool.show("b");
  complete({
    kind: "document",
    document: {
      id: "delayed",
      entry,
      text: "old",
      revision,
      bom: false,
      lineEndings: "LF",
    },
  });
  await opening;
  expect(reveal).not.toHaveBeenCalled();
  expect(strip.textContent).not.toContain("hello.txt");
  pool.show("a");
  expect(strip.textContent).toContain("hello.txt");
  pool.dispose();
});

test("protecting drafts captures dirty buffers without publishing them", async () => {
  const pool = new EditorPool(
    1,
    document.createElement("div"),
    document.createElement("div"),
    vi.fn(),
    vi.fn(),
    vi.fn(),
  );
  pool.show("s");
  await pool.open("s", entry);
  fake.models[0].setValue("draft only");
  vi.mocked(choose).mockResolvedValue("protect");
  expect(await pool.guard(undefined, true)).toBe(true);
  expect(pool.capture()).toEqual([
    { sessionId: "s", documentId: "doc", text: "draft only" },
  ]);
  expect(api.saveDocument).not.toHaveBeenCalled();
  pool.dispose();
});

test("a draft response after session closure cannot recreate an orphan tab", async () => {
  const strip = document.createElement("div");
  const pool = new EditorPool(
    1,
    strip,
    document.createElement("div"),
    vi.fn(),
    vi.fn(),
    vi.fn(),
  );
  pool.show("s");
  let complete!: (doc: api.RemoteDocument) => void;
  vi.mocked(draftRestore).mockReturnValueOnce(
    new Promise((resolve) => {
      complete = resolve;
    }),
  );
  const restore = pool.restore("s", "protected-draft");
  pool.closeSession("s");
  complete({
    id: "late-doc",
    entry,
    text: "protected",
    revision,
    bom: false,
    lineEndings: "LF",
  });
  await restore;
  expect(fake.models).toHaveLength(0);
  expect(pool.capture()).toEqual([]);
  expect(api.releaseFiles).toHaveBeenCalledWith(1, "s", "late-doc");
  pool.dispose();
});
