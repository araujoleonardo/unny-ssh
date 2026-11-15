import { terminalAtDirectory, type RemoteEntry } from "../../ipc/files";
import {
  exec as dockerExec,
  type Resource as DockerResource,
} from "../../ipc/docker";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { Channel } from "@tauri-apps/api/core";
import * as api from "../../ipc/connections";
import { safeError } from "../../ipc/client";
import type { TerminalEvent } from "../../ipc/contracts";
import "@xterm/xterm/css/xterm.css";
import { TerminalOrder } from "./protocol";
interface View {
  localId: string;
  id?: string;
  sessionId: string;
  name: string;
  terminal: Terminal;
  fit: FitAddon;
  element: HTMLElement;
  observer: ResizeObserver;
  order: TerminalOrder;
  state: "opening" | "active" | "closed" | "error";
  lastData: number;
  dispose: () => void;
}
export class TerminalPool {
  private views = new Map<string, View>();
  private active = new Map<string, string>();
  private counters = new Map<string, number>();
  private session: string | undefined;
  private disposed = false;
  constructor(
    private generation: number,
    private tabs: HTMLElement,
    private surface: HTMLElement,
    private report: (text: string) => void,
  ) {}
  show(sessionId: string | undefined) {
    this.session = sessionId;
    this.draw();
  }
  has(sessionId: string) {
    return [...this.views.values()].some((v) => v.sessionId === sessionId);
  }
  async open(
    sessionId: string,
    container?: DockerResource,
    directory?: RemoteEntry,
  ) {
    if (
      this.views.size >= 32 ||
      [...this.views.values()].filter((v) => v.sessionId === sessionId)
        .length >= 8
    ) {
      this.report(
        "Feche uma aba antes de abrir outro terminal (8 por sessão, 32 no total).",
      );
      return;
    }
    const number = (this.counters.get(sessionId) ?? 0) + 1;
    this.counters.set(sessionId, number);
    const element = document.createElement("div");
    element.className = "live-terminal";
    this.surface.append(element);
    const terminal = new Terminal({
      scrollback: 5000,
      fontSize: 12,
      fontFamily: "Menlo, Consolas, monospace",
      disableStdin: true,
      theme: { background: "#181a1c", foreground: "#bcbec4", green: "#a1d4b3" },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(element);
    const order = new TerminalOrder(sessionId, this.generation);
    const localId = crypto.randomUUID();
    const view: View = {
      localId,
      sessionId,
      name: container
        ? `Container · ${container.name}`
        : directory
          ? `Terminal · ${directory.name}`
          : `Terminal ${number}`,
      terminal,
      fit,
      element,
      order,
      state: "opening",
      lastData: Date.now(),
      observer: new ResizeObserver(() => {}),
      dispose: () => {},
    };
    this.views.set(localId, view);
    this.active.set(sessionId, localId);
    this.draw();
    // Remote clipboard and hyperlink escape sequences are consumed without action.
    const osc52 = terminal.parser.registerOscHandler(52, () => true);
    const osc8 = terminal.parser.registerOscHandler(8, () => true);
    let queuedBytes = 0;
    let pendingInputs = 0;
    let inputTail = Promise.resolve();
    let alive = true;
    const send = (bytes: Uint8Array) => {
      if (!alive || view.state !== "active" || !view.id) return;
      if (bytes.length + queuedBytes > 256 * 1024 || pendingInputs >= 64) {
        this.report(
          "A entrada excede 256 KiB. Divida a colagem em partes menores.",
        );
        return;
      }
      queuedBytes += bytes.length;
      pendingInputs++;
      inputTail = inputTail
        .then(async () => {
          for (let offset = 0; offset < bytes.length; offset += 16384) {
            if (!alive || view.state !== "active") break;
            await api.terminalAction(this.generation, sessionId, view.id!, {
              action: "write",
              bytes: Array.from(bytes.subarray(offset, offset + 16384)),
            });
          }
        })
        .catch((error) => {
          view.state = "error";
          terminal.options.disableStdin = true;
          this.report(safeError(error));
          this.draw();
        })
        .finally(() => {
          queuedBytes -= bytes.length;
          pendingInputs--;
        });
    };
    const data = terminal.onData((text) =>
      send(new TextEncoder().encode(text)),
    );
    const binary = terminal.onBinary((text) =>
      send(Uint8Array.from(text, (c) => c.charCodeAt(0))),
    );
    let resizeTimer: ReturnType<typeof setTimeout> | undefined;
    const resize = terminal.onResize(({ cols, rows }) => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        if (view.id && view.state === "active")
          void api
            .terminalAction(this.generation, sessionId, view.id, {
              action: "resize",
              cols: Math.min(500, Math.max(2, cols)),
              rows: Math.min(300, Math.max(1, rows)),
            })
            .catch((error) => this.report(safeError(error)));
      }, 80);
    });
    terminal.attachCustomKeyEventHandler((event) => {
      const copy =
        (event.metaKey && !event.shiftKey && event.key.toLowerCase() === "c") ||
        (event.ctrlKey && event.shiftKey && event.key.toLowerCase() === "c");
      if (copy && terminal.hasSelection()) {
        if (event.type === "keydown")
          void api
            .clipboardWrite(this.generation, terminal.getSelection())
            .catch(() =>
              this.report(
                "Não foi possível copiar. Use o menu de edição do sistema.",
              ),
            );
        return false;
      }
      return true;
    });
    view.observer = new ResizeObserver(() => {
      if (element.clientWidth && element.clientHeight) {
        fit.fit();
      }
    });
    view.observer.observe(element);
    view.dispose = () => {
      alive = false;
      clearTimeout(resizeTimer);
      order.cancel();
      view.observer.disconnect();
      data.dispose();
      binary.dispose();
      resize.dispose();
      osc52.dispose();
      osc8.dispose();
      terminal.dispose();
      element.remove();
    };
    const channel = new Channel<TerminalEvent>();
    channel.onmessage = (event) => {
      if (!alive || !order.accept(event)) return;
      view.id = event.terminalId;
      if (event.kind === "data") {
        if (view.state === "opening") {
          view.state = "active";
          terminal.options.disableStdin = false;
        }
        view.lastData = Date.now();
        terminal.write(Uint8Array.from(event.bytes), () => {
          if (alive && view.state === "active")
            void api
              .terminalAction(this.generation, sessionId, event.terminalId, {
                action: "ack",
                sequence: event.sequence,
              })
              .catch((error) => this.report(safeError(error)));
        });
      } else {
        view.state = event.kind === "error" ? "error" : "closed";
        terminal.options.disableStdin = true;
        terminal.writeln(
          `\r\n[Terminal ${event.kind === "error" ? "interrompido por erro" : "encerrado"}${event.exitStatus === null ? "" : ` · saída ${event.exitStatus}`}]`,
        );
        this.draw();
      }
    };
    try {
      const id = await (container
        ? dockerExec(this.generation, sessionId, container.handle, channel)
        : directory
          ? terminalAtDirectory(
              this.generation,
              sessionId,
              directory.id,
              channel,
            )
          : api.terminalOpen(this.generation, sessionId, 80, 24, channel));
      if (!alive || this.disposed) {
        void api
          .terminalAction(this.generation, sessionId, id, { action: "close" })
          .catch(() => {});
        return;
      }
      view.id = id;
      if (view.state === "opening") {
        view.state = "active";
        terminal.options.disableStdin = false;
      }
      this.draw();
      if (element.clientWidth && element.clientHeight) fit.fit();
      terminal.focus();
    } catch (error) {
      if (alive) {
        view.state = "error";
        terminal.writeln("Não foi possível abrir este terminal.");
        this.report(safeError(error));
        this.draw();
      }
    }
  }
  private draw() {
    this.tabs.replaceChildren();
    for (const view of this.views.values()) {
      const selected =
        view.sessionId === this.session &&
        this.active.get(this.session) === view.localId;
      view.element.hidden = !selected;
      if (view.sessionId !== this.session) continue;
      const group = document.createElement("div");
      group.className = "terminal-tab";
      const button = document.createElement("button");
      button.textContent = `${view.name} · ${view.state === "opening" ? "abrindo" : view.state === "closed" ? "encerrado" : view.state === "error" ? "erro" : Date.now() - view.lastData > 10000 ? "ocioso" : "ativo"}`;
      button.setAttribute("aria-pressed", String(selected));
      button.dataset.terminalTab = view.localId;
      button.onclick = () => {
        this.active.set(view.sessionId, view.localId);
        this.draw();
        view.terminal.focus();
      };
      const close = document.createElement("button");
      close.textContent = "×";
      close.setAttribute("aria-label", `Fechar ${view.name}`);
      close.onclick = () => this.close(view.localId);
      group.append(button, close);
      this.tabs.append(group);
      if (selected && view.element.clientWidth && view.element.clientHeight)
        view.fit.fit();
    }
    this.surface.classList.toggle(
      "terminal-empty",
      ![...this.views.values()].some((v) => v.sessionId === this.session),
    );
  }
  refresh() {
    for (const view of this.views.values()) {
      const button = this.tabs.querySelector<HTMLElement>(
        `[data-terminal-tab="${view.localId}"]`,
      );
      if (button)
        button.textContent = `${view.name} · ${view.state === "opening" ? "abrindo" : view.state === "closed" ? "encerrado" : view.state === "error" ? "erro" : Date.now() - view.lastData > 10000 ? "ocioso" : "ativo"}`;
    }
  }
  private current() {
    const id = this.session ? this.active.get(this.session) : undefined;
    return id ? this.views.get(id) : undefined;
  }
  interrupt() {
    const view = this.current();
    if (view?.id && view.state === "active")
      void api
        .terminalAction(this.generation, view.sessionId, view.id, {
          action: "write",
          bytes: [3],
        })
        .catch((error) => this.report(safeError(error)));
  }
  copy() {
    const view = this.current();
    if (view?.terminal.hasSelection())
      void api
        .clipboardWrite(this.generation, view.terminal.getSelection())
        .catch(() =>
          this.report("Não foi possível acessar a área de transferência."),
        );
  }
  async paste() {
    const view = this.current();
    if (!view || view.state !== "active") return;
    try {
      const text = await api.clipboardRead(this.generation);
      if (text.length > 256 * 1024) {
        this.report("Divida a colagem em partes de até 256 KiB.");
        return;
      }
      if (this.current() === view && view.state === "active")
        view.terminal.paste(text);
    } catch {
      this.report(
        "Não foi possível ler a área de transferência. Use o atalho de colagem do sistema.",
      );
    }
  }
  close(localId: string) {
    const view = this.views.get(localId);
    if (!view) return;
    if (view.id && view.state === "active")
      void api
        .terminalAction(this.generation, view.sessionId, view.id, {
          action: "close",
        })
        .catch(() => {});
    view.dispose();
    this.views.delete(localId);
    if (this.active.get(view.sessionId) === localId) {
      const next = [...this.views.values()].find(
        (v) => v.sessionId === view.sessionId,
      );
      if (next) this.active.set(view.sessionId, next.localId);
      else this.active.delete(view.sessionId);
    }
    this.draw();
  }
  closeSession(id: string) {
    for (const view of [...this.views.values()].filter(
      (v) => v.sessionId === id,
    ))
      this.close(view.localId);
  }
  dispose() {
    this.disposed = true;
    for (const view of [...this.views.values()]) this.close(view.localId);
  }
}
