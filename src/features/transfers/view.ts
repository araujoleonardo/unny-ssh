import { Channel } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import * as api from "../../ipc/files";
import type {
  TransferSnapshot,
  LocalSelection,
  RemoteEntry,
} from "../../ipc/files";
import { safeError } from "../../ipc/client";
import { choose, askName } from "../files/dialogs";
const labels: Record<TransferSnapshot["state"], string> = {
  queued: "Na fila",
  preparing: "Preparando",
  running: "Transferindo",
  waiting_collision: "Escolha necessária",
  completed: "Concluído",
  failed: "Falhou",
  cancelled: "Cancelado",
  skipped: "Ignorado",
};
export function bytes(n: number) {
  return n < 1024
    ? `${n} B`
    : n < 1024 ** 2
      ? `${(n / 1024).toFixed(1)} KiB`
      : n < 1024 ** 3
        ? `${(n / 1024 ** 2).toFixed(1)} MiB`
        : `${(n / 1024 ** 3).toFixed(2)} GiB`;
}
export function activeTransfer(snapshot: TransferSnapshot) {
  return ["queued", "preparing", "running", "waiting_collision"].includes(
    snapshot.state,
  );
}
export class TransferPanel {
  private snapshots = new Map<string, TransferSnapshot>();
  private decisions = new Set<string>();
  private owner: string | undefined;
  private alive = true;
  private unlisten: UnlistenFn[] = [];
  constructor(
    private generation: number,
    private container: HTMLElement,
    private report: (message: string) => void,
    private uploaded: (session: string) => void,
    private uploadTarget: () =>
      { session: string; directory: RemoteEntry } | undefined,
  ) {
    container.innerHTML =
      '<div class="transfer-heading"><h2>Transferências</h2><small>Downloads saem da proteção do cofre</small></div><div class="transfer-list" aria-live="polite"></div>';
    void listen<LocalSelection[]>("local-files-selected", (event) => {
      const target = this.uploadTarget();
      if (target && this.alive)
        void choose(
          "Enviar arquivos soltos",
          `${event.payload.length} arquivo(s) para o diretório remoto:\n${target.directory.path}`,
          [
            { label: "Cancelar", value: "cancel" },
            { label: "Enviar", value: "upload" },
          ],
        ).then((action) => {
          if (action === "upload" && this.alive)
            return this.sendSelections(
              target.session,
              target.directory,
              event.payload,
            );
        });
      else if (this.alive)
        this.report(
          "Selecione uma pasta em uma sessão SFTP antes de soltar arquivos.",
        );
    }).then((stop) => {
      if (this.alive) this.unlisten.push(stop);
      else stop();
    });
    void listen<unknown>("local-files-error", (event) =>
      this.report(safeError(event.payload)),
    ).then((stop) => {
      if (this.alive) this.unlisten.push(stop);
      else stop();
    });
  }
  show(session: string | undefined) {
    this.owner = session;
    this.draw();
  }
  hasActive(session?: string) {
    return [...this.snapshots.values()].some(
      (s) => (!session || s.sessionId === session) && activeTransfer(s),
    );
  }
  async cancelAll(session?: string) {
    await Promise.all(
      [...this.snapshots.values()]
        .filter(
          (s) => (!session || s.sessionId === session) && activeTransfer(s),
        )
        .map((s) =>
          api
            .transferAction(this.generation, s.sessionId, s.id, {
              action: "cancel",
            })
            .catch(() => {}),
        ),
    );
  }
  private channel(session: string) {
    const channel = new Channel<TransferSnapshot>();
    channel.onmessage = (snapshot) => {
      if (
        !this.alive ||
        snapshot.generation !== this.generation ||
        snapshot.sessionId !== session
      )
        return;
      const previous = this.snapshots.get(snapshot.id);
      if (previous && snapshot.sequence <= previous.sequence) return;
      this.snapshots.set(snapshot.id, snapshot);
      if (snapshot.state === "completed" && snapshot.kind === "upload")
        this.uploaded(session);
      if (snapshot.collision && !this.decisions.has(snapshot.collision.token)) {
        this.decisions.add(snapshot.collision.token);
        void this.resolve(snapshot).finally(() =>
          this.decisions.delete(snapshot.collision!.token),
        );
      }
      if (activeTransfer(snapshot))
        void api
          .transferAction(this.generation, session, snapshot.id, {
            action: "ack",
            sequence: snapshot.sequence,
          })
          .catch(() => {});
      this.draw();
    };
    return channel;
  }
  async download(session: string, entry: RemoteEntry) {
    try {
      const destination = await api.selectDestination(
        this.generation,
        session,
        entry.id,
      );
      if (!destination || !this.alive) return;
      const snapshot = await api.download(
        this.generation,
        session,
        entry.id,
        destination.id,
        this.channel(session),
      );
      if (this.alive && !this.snapshots.has(snapshot.id)) {
        this.snapshots.set(snapshot.id, snapshot);
        this.draw();
      }
    } catch (error) {
      if (this.alive) this.report(safeError(error));
    }
  }
  async pickUploads(session: string, directory: RemoteEntry) {
    try {
      const selected = await api.selectUploads(this.generation, session);
      if (this.alive) await this.sendSelections(session, directory, selected);
    } catch (error) {
      this.report(safeError(error));
    }
  }
  private async sendSelections(
    session: string,
    directory: RemoteEntry,
    selections: LocalSelection[],
  ) {
    for (const source of selections) {
      if (!this.alive) return;
      try {
        const snapshot = await api.upload(
          this.generation,
          session,
          directory.id,
          source.id,
          this.channel(session),
        );
        if (!this.snapshots.has(snapshot.id))
          this.snapshots.set(snapshot.id, snapshot);
        this.draw();
      } catch (error) {
        this.report(
          `${source.name}: ${safeError(error)} Os demais arquivos continuam disponíveis no seletor.`,
        );
      }
    }
  }
  private async resolve(snapshot: TransferSnapshot): Promise<void> {
    const collision = snapshot.collision!;
    const action = await choose(
      "O destino já existe",
      `${collision.target}\n\n${collision.nonAtomic ? "O servidor não oferece substituição atômica. Sobrescrever pode deixar o destino parcial se a conexão cair." : "Escolha como tratar esta colisão."}`,
      [
        { label: "Ignorar", value: "skip" },
        { label: "Renomear", value: "rename" },
        { label: "Sobrescrever", value: "overwrite", danger: true },
      ],
    );
    if (!this.alive) return;
    let decision: api.CollisionDecision = { action: "skip" };
    if (action === "rename") {
      const name = await askName(
        "Renomear destino",
        "Use um nome simples para o novo arquivo.",
        snapshot.name,
      );
      if (!name) {
        this.decisions.delete(collision.token);
        return this.resolve(snapshot);
      }
      decision = { action: "rename", name };
    } else if (action === "overwrite")
      decision = { action: "overwrite", allow_non_atomic: collision.nonAtomic };
    try {
      await api.transferAction(
        this.generation,
        snapshot.sessionId,
        snapshot.id,
        { action: "resolve", token: collision.token, decision },
      );
    } catch (error) {
      this.report(safeError(error));
    }
  }
  private draw() {
    if (!this.alive) return;
    for (const [id, snapshot] of this.snapshots) {
      if (this.snapshots.size <= 64) break;
      if (!activeTransfer(snapshot)) this.snapshots.delete(id);
    }
    const list = this.container.querySelector<HTMLElement>(".transfer-list")!;
    list.replaceChildren();
    const items = [...this.snapshots.values()]
      .filter((s) => s.sessionId === this.owner)
      .slice(-64)
      .reverse();
    this.container.hidden = items.length === 0;
    for (const snapshot of items) {
      const row = document.createElement("div");
      row.className = `transfer-row transfer-${snapshot.state}`;
      const heading = document.createElement("strong");
      heading.textContent = `${snapshot.kind === "upload" ? "↑" : "↓"} ${snapshot.name} · ${labels[snapshot.state]}`;
      const target = document.createElement("small");
      target.textContent = snapshot.destination;
      const progress = document.createElement("progress");
      if (snapshot.total !== null && snapshot.total > 0) {
        progress.max = snapshot.total;
        progress.value = snapshot.processed;
      } else if (!activeTransfer(snapshot)) {
        progress.max = 1;
        progress.value = snapshot.state === "completed" ? 1 : 0;
      }
      progress.setAttribute(
        "aria-label",
        `${snapshot.name}: ${labels[snapshot.state]}`,
      );
      const amount = document.createElement("small");
      amount.textContent = `${bytes(snapshot.processed)}${snapshot.total !== null ? ` / ${bytes(snapshot.total)}` : ""}${snapshot.ignoredCount ? ` · ${snapshot.ignoredCount} ignorado(s)` : ""}`;
      row.append(heading, target, progress, amount);
      if (snapshot.message) {
        const text = document.createElement("p");
        text.textContent = snapshot.message;
        row.append(text);
      }
      if (snapshot.ignored.length || snapshot.residues.length) {
        const detail = document.createElement("details"),
          summary = document.createElement("summary"),
          text = document.createElement("pre");
        summary.textContent = "Ignorados e resíduos";
        text.textContent = [
          ...snapshot.ignored.map((p) => `Ignorado: ${p}`),
          ...snapshot.residues.map((p) => `Incompleto: ${p}`),
        ].join("\n");
        detail.append(summary, text);
        row.append(detail);
      }
      if (activeTransfer(snapshot)) {
        const cancel = document.createElement("button");
        cancel.textContent = "Cancelar";
        cancel.onclick = () =>
          void api
            .transferAction(this.generation, snapshot.sessionId, snapshot.id, {
              action: "cancel",
            })
            .catch((error) => this.report(safeError(error)));
        row.append(cancel);
      }
      list.append(row);
    }
  }
  dispose() {
    this.alive = false;
    this.unlisten.forEach((stop) => stop());
    void this.cancelAll();
  }
}
