import { draftRestore, type DirtyBuffer } from "../../ipc/lifecycle";
import * as monaco from "monaco-editor/editor/editor.api.js";
import "monaco-editor/editor/contrib/find/browser/findController.js";

import "monaco-editor/languages/definitions/yaml/register.js";
import "monaco-editor/languages/definitions/javascript/register.js";
import "./preview";
import * as api from "../../ipc/files";
import type { RemoteDocument, RemoteEntry } from "../../ipc/files";
import { safeError } from "../../ipc/client";
import { choose } from "../files/dialogs";
type Tab = {
  session: string;
  doc: RemoteDocument;
  model: monaco.editor.ITextModel;
  clean: number;
  busy: boolean;
  change: monaco.IDisposable;
};
function languageFor(name: string) {
  return (
    (
      {
        sh: "shell",
        json: "json",
        yaml: "yaml",
        yml: "yaml",
        js: "javascript",
        ts: "typescript",
        css: "css",
        html: "html",
        md: "markdown",
      } as Record<string, string>
    )[name.split(".").pop() ?? ""] ?? "plaintext"
  );
}
export class EditorPool {
  private tabs: Tab[] = [];
  private active?: Tab;
  private session?: string;
  private alive = true;
  private pending = new Set<string>();
  private editor: monaco.editor.IStandaloneCodeEditor;
  constructor(
    private generation: number,
    private strip: HTMLElement,
    host: HTMLElement,
    private report: (s: string) => void,
    private reveal: () => void,
    private download: (s: string, e: RemoteEntry) => void,
  ) {
    this.editor = monaco.editor.create(host, {
      theme: "islands",
      automaticLayout: true,
      minimap: { enabled: false },
      fontSize: 13,
      scrollBeyondLastLine: false,
    });
    this.editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => {
      if (this.active) void this.save(this.active);
    });
  }
  private dirty(t: Tab) {
    return t.clean !== t.model.getAlternativeVersionId();
  }
  async open(session: string, entry: RemoteEntry) {
    const existing = this.tabs.find(
      (t) => t.session === session && t.doc.entry.id === entry.id,
    );
    if (existing) {
      if (session === this.session) this.select(existing);
      return;
    }
    const key = `${session}/${entry.id}`;
    if (this.pending.has(key)) return;
    this.pending.add(key);
    try {
      const reply = await api.openDocument(this.generation, session, entry.id);
      if (!this.alive || !this.pending.has(key)) {
        if (reply.kind === "document")
          void api
            .releaseFiles(this.generation, session, reply.document.id)
            .catch(() => {});
        return;
      }
      if (reply.kind === "restricted") {
        if (
          (await choose("Edição indisponível", reply.reason, [
            { label: "Cancelar", value: "cancel" },
            { label: "Baixar arquivo", value: "download" },
          ])) === "download"
        )
          this.download(session, entry);
        return;
      }
      if (this.tabs.length >= 32) {
        this.report("Feche uma aba antes de abrir mais arquivos (limite 32).");
        await api.releaseFiles(this.generation, session, reply.document.id);
        return;
      }
      const model = monaco.editor.createModel(
        reply.document.text,
        languageFor(entry.name),
      );
      const tab: Tab = {
        session,
        doc: reply.document,
        model,
        clean: model.getAlternativeVersionId(),
        busy: false,
        change: { dispose() {} },
      };
      tab.change = model.onDidChangeContent(() => this.draw());
      this.tabs.push(tab);
      if (session === this.session) this.select(tab);
      else this.draw();
    } catch (error) {
      this.report(safeError(error));
    } finally {
      this.pending.delete(key);
    }
  }
  capture(): DirtyBuffer[] {
    return this.tabs
      .filter((t) => this.dirty(t))
      .map((t) => ({
        sessionId: t.session,
        documentId: t.doc.id,
        text: t.model.getValue(),
      }));
  }
  async restore(session: string, id: string) {
    const key = `${session}/draft:${id}`;
    if (this.pending.has(key)) return;
    this.pending.add(key);
    try {
      const doc = await draftRestore(this.generation, session, id);
      if (!this.alive || !this.pending.has(key)) {
        void api.releaseFiles(this.generation, session, doc.id).catch(() => {});
        return;
      }
      if (this.tabs.length >= 32) {
        await api.releaseFiles(this.generation, session, doc.id);
        this.report("Feche uma aba antes de recuperar mais documentos.");
        return;
      }
      const model = monaco.editor.createModel(
        doc.text,
        languageFor(doc.entry.name),
      );
      const tab: Tab = {
        session,
        doc,
        model,
        clean: -1,
        busy: false,
        change: { dispose() {} },
      };
      tab.change = model.onDidChangeContent(() => this.draw());
      this.tabs.push(tab);
      if (session === this.session) this.select(tab);
      else this.draw();
      this.report(
        "Rascunho recuperado em memória. Salvar exige uma ação explícita; a revisão original será comparada.",
      );
    } catch (error) {
      this.report(safeError(error));
    } finally {
      this.pending.delete(key);
    }
  }
  private select(tab: Tab) {
    this.active = tab;
    this.editor.setModel(tab.model);
    this.reveal();
    this.draw();
    this.editor.focus();
  }
  show(session?: string) {
    this.session = session;
    if (this.active?.session !== session) {
      this.active = this.tabs.find((t) => t.session === session);
      this.editor.setModel(this.active?.model ?? null);
    }
    this.draw();
  }
  async save(tab = this.active): Promise<boolean> {
    if (!tab || tab.busy) return false;
    tab.busy = true;
    this.draw();
    const text = tab.model.getValue(),
      version = tab.model.getAlternativeVersionId();
    let token: string | undefined;
    try {
      for (let attempt = 0; attempt < 8; attempt++) {
        const reply = await api.saveDocument(
          this.generation,
          tab.session,
          tab.doc.id,
          tab.doc.revision,
          text,
          token,
        );
        if (!this.alive) return false;
        if (reply.kind === "saved") {
          tab.doc.revision = reply.revision;
          tab.clean = version;
          this.report("Arquivo salvo no servidor.");
          return true;
        }
        if (reply.kind === "failed") {
          this.report(
            `${safeError(reply.error)}${reply.residue ? ` Resíduo remoto: ${reply.residue}` : ""}`,
          );
          return false;
        }
        if (reply.kind === "non_atomic") {
          if (
            (await choose(
              "Gravação sem substituição atômica",
              "O servidor não oferece rename atômico. Uma queda pode deixar o destino parcial. Autorizar esta gravação?",
              [
                { label: "Cancelar", value: "cancel" },
                {
                  label: "Gravar mesmo assim",
                  value: "overwrite",
                  danger: true,
                },
              ],
            )) !== "overwrite"
          )
            return false;
          token = reply.token;
          continue;
        }
        const action = await choose(
          "Arquivo alterado no servidor",
          reply.missing
            ? "O arquivo foi removido remotamente."
            : "A revisão remota mudou. Seu conteúdo local continua preservado.",
          [
            { label: "Cancelar", value: "cancel" },
            { label: "Recarregar", value: "reload" },
            { label: "Sobrescrever", value: "overwrite", danger: true },
          ],
        );
        if (action === "reload") {
          const fresh = await api.openDocument(
            this.generation,
            tab.session,
            tab.doc.entry.id,
          );
          if (fresh.kind !== "document") {
            this.report(fresh.reason);
            return false;
          }
          await api.releaseFiles(this.generation, tab.session, tab.doc.id);
          tab.doc = fresh.document;
          tab.model.setValue(fresh.document.text);
          tab.clean = tab.model.getAlternativeVersionId();
          return true;
        }
        if (action !== "overwrite") return false;
        token = reply.token;
      }
      this.report("O destino continua mudando. Tente salvar novamente.");
      return false;
    } catch (error) {
      this.report(safeError(error));
      return false;
    } finally {
      tab.busy = false;
      this.draw();
    }
  }
  async guard(session?: string, allowDrafts = false): Promise<boolean> {
    const affected = this.tabs.filter(
      (t) => (!session || t.session === session) && this.dirty(t),
    );
    if (this.tabs.some((t) => (!session || t.session === session) && t.busy)) {
      this.report("Aguarde a gravação em andamento.");
      return false;
    }
    if (!affected.length) return true;
    const action = await choose(
      "Alterações não salvas",
      `${affected.length} arquivo(s) têm alterações locais.`,
      [
        { label: "Cancelar", value: "cancel" },
        { label: "Descartar alterações", value: "discard", danger: true },
        { label: "Salvar todos", value: "save" },
        ...(allowDrafts
          ? [{ label: "Proteger rascunhos ao sair", value: "protect" }]
          : []),
      ],
    );
    if (action === "protect") return true;
    if (action === "discard") {
      for (const tab of affected) this.remove(tab);
      this.show(this.session);
      return true;
    }
    if (action !== "save") return false;
    for (const tab of affected) if (!(await this.save(tab))) return false;
    return !affected.some((t) => this.dirty(t));
  }
  private async close(tab: Tab) {
    if (tab.busy) return;
    if (this.dirty(tab)) {
      const action = await choose("Fechar arquivo", tab.doc.entry.path, [
        { label: "Cancelar", value: "cancel" },
        { label: "Descartar", value: "discard", danger: true },
        { label: "Salvar", value: "save" },
      ]);
      if (action === "save") {
        if (!(await this.save(tab)) || this.dirty(tab)) return;
      } else if (action !== "discard") return;
    }
    this.remove(tab);
    this.show(this.session);
  }
  private remove(tab: Tab) {
    tab.change.dispose();
    tab.model.dispose();
    this.tabs = this.tabs.filter((t) => t !== tab);
    void api
      .releaseFiles(this.generation, tab.session, tab.doc.id)
      .catch(() => {});
  }
  closeSession(session: string) {
    for (const key of this.pending)
      if (key.startsWith(`${session}/`)) this.pending.delete(key);
    for (const tab of [...this.tabs])
      if (tab.session === session) this.remove(tab);
    this.show(this.session);
  }
  private draw() {
    if (!this.alive) return;
    this.strip.replaceChildren();
    for (const tab of this.tabs.filter((t) => t.session === this.session)) {
      const group = document.createElement("div");
      group.className = "document-tab";
      const select = document.createElement("button"),
        close = document.createElement("button");
      select.className = tab === this.active ? "tab-active" : "";
      select.textContent = `${tab.doc.entry.name}${this.dirty(tab) ? " ●" : ""}${tab.busy ? " …" : ""}`;
      select.title = tab.doc.entry.path;
      select.onclick = () => this.select(tab);
      close.textContent = "×";
      close.setAttribute("aria-label", `Fechar ${tab.doc.entry.name}`);
      close.onclick = () => void this.close(tab);
      group.append(select, close);
      this.strip.append(group);
    }
    if (this.active && this.active.session === this.session) {
      const save = document.createElement("button");
      save.textContent = "Salvar";
      save.disabled = this.active.busy;
      save.onclick = () => void this.save();
      this.strip.append(save);
    }
  }
  dispose() {
    this.alive = false;
    this.editor.dispose();
    for (const tab of [...this.tabs]) this.remove(tab);
  }
}
