// @vitest-environment jsdom
import { expect, test, vi } from "vitest";
const fixture = vi.hoisted(() => ({
  hooks: new Map<number, () => boolean>(),
  keyHandler: undefined as undefined | ((event: KeyboardEvent) => boolean),
  paste: vi.fn(),
}));
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    options = { disableStdin: true };
    parser = {
      registerOscHandler: (code: number, handler: () => boolean) => {
        fixture.hooks.set(code, handler);
        return { dispose() {} };
      },
    };
    open() {}
    loadAddon() {}
    focus() {}
    dispose() {}
    writeln() {}
    hasSelection() {
      return true;
    }
    getSelection() {
      return "synthetic selection";
    }
    paste = fixture.paste;
    onData() {
      return { dispose() {} };
    }
    onBinary() {
      return { dispose() {} };
    }
    onResize() {
      return { dispose() {} };
    }
    attachCustomKeyEventHandler(handler: (event: KeyboardEvent) => boolean) {
      fixture.keyHandler = handler;
    }
  },
}));
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage: unknown;
  },
}));
vi.mock("../../ipc/connections", () => ({
  terminalOpen: vi.fn().mockResolvedValue("terminal"),
  terminalAction: vi.fn().mockResolvedValue(undefined),
  clipboardWrite: vi.fn().mockResolvedValue(undefined),
  clipboardRead: vi.fn().mockResolvedValue("synthetic paste"),
}));
vi.mock("../../ipc/client", () => ({ safeError: () => "failure" }));
import { TerminalPool } from "./live";
import * as api from "../../ipc/connections";
test("clipboard actions use native IPC explicitly while remote OSC is ignored", async () => {
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  document.body.innerHTML = '<div id="tabs"></div><div id="surface"></div>';
  const pool = new TerminalPool(
    7,
    document.querySelector("#tabs")!,
    document.querySelector("#surface")!,
    vi.fn(),
  );
  pool.show("session");
  await pool.open("session");
  expect(fixture.hooks.get(52)!()).toBe(true);
  expect(fixture.hooks.get(8)!()).toBe(true);
  expect(api.clipboardWrite).not.toHaveBeenCalled();
  expect(api.clipboardRead).not.toHaveBeenCalled();
  pool.copy();
  expect(api.clipboardWrite).toHaveBeenCalledWith(7, "synthetic selection");
  fixture.keyHandler!(
    new KeyboardEvent("keydown", { key: "c", ctrlKey: true, shiftKey: true }),
  );
  expect(api.clipboardWrite).toHaveBeenCalledTimes(2);
  await pool.paste();
  expect(api.clipboardRead).toHaveBeenCalledWith(7);
  expect(fixture.paste).toHaveBeenCalledWith("synthetic paste");
  pool.dispose();
  vi.unstubAllGlobals();
});
