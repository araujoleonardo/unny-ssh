import { Popup } from "../../ui/popup";
import { icon } from "../../ui/icons";
import * as api from "../../ipc/files";
import type { RemoteEntry, Listing } from "../../ipc/files";
import type { SessionInfo } from "../../ipc/contracts";
import { safeError } from "../../ipc/client";
import { askName, choose } from "./dialogs";
// Natural name order keeps file2 before file10 and ignores case for grouping.
const names = new Intl.Collator(undefined, {
  numeric: true,
  sensitivity: "base",
});
type Node = {
  parent?: Node;
  entry: RemoteEntry;
  depth: number;
  open: boolean;
  children: Node[];
  cursor: string | null;
  loaded: boolean;
  busy: boolean;
};
export class FileBrowser {
  private menu: Popup;
  private session?: SessionInfo;
  private trees = new Map<string, Node>();
  private selected?: Node;
  private parents = new Map<string, RemoteEntry | null>();
  private alive = true;
  private loading = new Set<string>();
  constructor(
    private generation: number,
    private host: HTMLElement,
    private open: (s: string, e: RemoteEntry) => void,
    private download: (s: string, e: RemoteEntry) => void,
    private upload: (s: string, e: RemoteEntry) => void,
    private report: (s: string) => void,
    private terminal?: (s: string, e: RemoteEntry) => void,
  ) {
    host.innerHTML = `<div class="file-actions"><button data-action="up" title="Pasta pai" aria-label="Pasta pai">${icon("chevron")}</button><button data-action="refresh" title="Atualizar" aria-label="Atualizar">${icon("refresh")}</button><button data-action="file" title="Novo arquivo" aria-label="Novo arquivo">${icon("plus")}</button><button data-action="upload" title="Enviar arquivos" aria-label="Enviar arquivos">${icon("upload")}</button></div><p class="remote-path"></p><div class="file-viewport" role="tree" aria-label="Arquivos remotos" tabindex="0"><div class="file-rows"></div></div><p class="file-message" role="status"></p>`;
    const panel = document.createElement("div");
    panel.className = "file-context-menu";
    panel.setAttribute("role", "menu");
    panel.setAttribute("aria-label", "Ações da entrada remota");
    host.append(panel);
    this.menu = new Popup(panel);
    host.querySelector<HTMLElement>(".file-viewport")!.onscroll = () =>
      this.draw();
    host
      .querySelectorAll<HTMLButtonElement>("[data-action]")
      .forEach((b) => (b.onclick = () => void this.action(b.dataset.action!)));
  }
  target() {
    const s = this.session;
    if (s?.state !== "connected" || !s.sftp) return;
    const root = this.trees.get(s.id);
    const n =
      this.selected?.entry.kind === "directory"
        ? this.selected
        : (this.selected?.parent ?? root);
    return n ? { session: s.id, directory: n.entry } : undefined;
  }
  show(session?: SessionInfo) {
    const changed = this.session?.id !== session?.id;
    this.session = session;
    if (session?.state !== "connected") this.menu.close(false);
    if (changed) {
      this.menu.close(false);
      this.selected = undefined;
      this.host.querySelector(".file-viewport")!.scrollTop = 0;
    }
    this.host
      .querySelectorAll<HTMLButtonElement>("[data-action]")
      .forEach(
        (b) => (b.disabled = session?.state !== "connected" || !session.sftp),
      );
    if (
      session?.state === "connected" &&
      session.sftp &&
      !this.trees.has(session.id)
    )
      void this.refresh();
    else this.draw();
  }
  async refresh(session = this.session?.id, directory?: string) {
    if (!session || this.loading.has(session)) return;
    this.menu.close(false);
    this.loading.add(session);
    const previous = this.trees.get(session);
    if (previous) await this.release(session, previous);
    try {
      const listing = await api.listFiles(
        this.generation,
        session,
        directory ?? previous?.entry.id,
      );
      if (!this.alive) return;
      const root = this.node(listing.directory, 0);
      root.open = true;
      this.apply(root, listing);
      this.trees.set(session, root);
      this.parents.set(session, listing.parent);
      if (this.session?.id === session) {
        this.selected = root;
        this.draw();
      }
    } catch (error) {
      if (this.alive) this.report(safeError(error));
    } finally {
      this.loading.delete(session);
    }
  }
  private node(entry: RemoteEntry, depth: number): Node {
    return {
      entry,
      depth,
      open: false,
      children: [],
      cursor: null,
      loaded: false,
      busy: false,
    };
  }
  private apply(node: Node, result: Listing) {
    node.children.push(
      ...result.entries.map((e) => ({
        ...this.node(e, node.depth + 1),
        parent: node,
      })),
    );
    // Merge each page into the existing siblings so folders from later pages
    // also move ahead of files without replacing expanded or selected nodes.
    node.children.sort((a, b) => {
      const directories =
        Number(b.entry.kind === "directory") -
        Number(a.entry.kind === "directory");
      return (
        directories ||
        names.compare(a.entry.name, b.entry.name) ||
        (a.entry.name < b.entry.name ? -1 : a.entry.name > b.entry.name ? 1 : 0)
      );
    });
    node.cursor = result.nextCursor;
    node.loaded = true;
  }
  private async expand(node: Node, more = false) {
    const session = this.session?.id;
    if (!session || node.busy) return;
    if (!more && node.loaded) {
      node.open = !node.open;
      this.draw();
      return;
    }
    node.open = true;
    node.busy = true;
    this.draw();
    try {
      const result = await api.listFiles(
        this.generation,
        session,
        node.entry.id,
        more ? (node.cursor ?? undefined) : undefined,
      );
      if (!this.alive) return;
      this.apply(node, result);
    } catch (error) {
      this.report(safeError(error));
    } finally {
      node.busy = false;
      this.draw();
    }
  }
  private message(text: string) {
    this.host.querySelector(".file-message")!.textContent = text;
  }
  private draw() {
    if (!this.alive) return;
    const session = this.session;
    const root = session && this.trees.get(session.id);
    const rows: { node: Node; more?: boolean }[] = [];
    const visit = (n: Node) => {
      rows.push({ node: n });
      if (n.open) {
        n.children.forEach(visit);
        if (n.cursor) rows.push({ node: n, more: true });
      }
    };
    if (root) visit(root);
    this.host.querySelector(".remote-path")!.textContent =
      this.selected?.entry.path ?? root?.entry.path ?? "";
    if (session?.state !== "connected")
      this.message("Conecte uma sessão SSH para navegar.");
    else if (!session.sftp)
      this.message("SFTP indisponível. O terminal continua disponível.");
    else if (!root) this.message("Carregando diretório inicial…");
    else
      this.message(
        `${rows.length} entradas carregadas · links não são seguidos`,
      );
    const viewport = this.host.querySelector<HTMLElement>(".file-viewport")!,
      surface = this.host.querySelector<HTMLElement>(".file-rows")!;
    surface.style.height = `${rows.length * 30}px`;
    surface.replaceChildren();
    const first = Math.max(0, Math.floor(viewport.scrollTop / 30) - 5),
      last = Math.min(
        rows.length,
        first + Math.ceil((viewport.clientHeight || 240) / 30) + 10,
      );
    for (let i = first; i < last; i++) {
      const { node, more } = rows[i];
      const button = document.createElement("button");
      button.className = `remote-file ${node === this.selected ? "selected" : ""}`;
      button.style.top = `${i * 30}px`;
      button.style.paddingLeft = `${8 + node.depth * 14}px`;
      button.innerHTML = more
        ? ""
        : `<span class="tree-chevron">${node.entry.kind === "directory" ? icon("chevron") : ""}</span>${icon(node.entry.kind === "directory" ? "folder" : "file")}`;
      button.classList.toggle("expanded", node.open);
      const label = document.createElement("span");
      label.textContent = more
        ? "Carregar mais…"
        : `${node.entry.name}${node.busy ? " …" : ""}`;
      button.append(label);
      button.title = node.entry.path;
      button.setAttribute("role", "treeitem");
      button.setAttribute("aria-level", String(node.depth + 1));
      button.setAttribute("aria-selected", String(node === this.selected));
      if (node.entry.kind === "directory")
        button.setAttribute("aria-expanded", String(node.open));
      button.onclick = () => {
        this.selected = node;
        if (more) void this.expand(node, true);
        else if (node.entry.kind === "directory") void this.expand(node);
        else this.draw();
      };
      button.ondblclick = () => {
        if (!more && node.entry.kind === "file" && session)
          this.open(session.id, node.entry);
      };
      button.oncontextmenu = (event) => {
        if (more) return;
        event.preventDefault();
        this.context(node, button, event.clientX, event.clientY);
      };
      button.onkeydown = (e) => {
        if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
          e.preventDefault();
          this.context(node, button);
          return;
        }

        if (e.key === "Enter" && node.entry.kind === "file" && session)
          this.open(session.id, node.entry);
      };
      surface.append(button);
    }
  }
  private context(node: Node, trigger: HTMLElement, x?: number, y?: number) {
    if (this.session?.state !== "connected") return;
    this.selected = node;
    this.host.querySelectorAll(".remote-file.selected").forEach((el) => {
      el.classList.remove("selected");
      el.setAttribute("aria-selected", "false");
    });
    trigger.classList.add("selected");
    trigger.setAttribute("aria-selected", "true");
    const panel = this.menu.panel;
    panel.replaceChildren();
    const add = (action: string, label: string, glyph: string) => {
      const b = document.createElement("button");
      b.dataset.action = action;
      b.setAttribute("role", "menuitem");
      b.innerHTML = icon(glyph);
      const text = document.createElement("span");
      text.textContent = label;
      b.append(text);
      b.classList.toggle("danger", action === "delete");
      b.onclick = () => {
        this.menu.close();
        void this.action(action);
      };
      panel.append(b);
    };
    if (node.entry.kind === "directory" && this.terminal)
      add("terminal", "Abrir no terminal", "terminal");
    if (node.entry.kind === "file") add("open", "Abrir no editor", "file");
    add(
      "download",
      node.entry.kind === "directory" ? "Baixar como ZIP" : "Baixar",
      "download",
    );
    if (node.entry.kind === "directory") {
      add("file", "Novo arquivo…", "file");
      add("folder", "Nova pasta…", "folder");
      add("upload", "Enviar arquivos…", "upload");
    }
    add("refresh", "Atualizar", "refresh");
    add("rename", "Renomear…", "edit");
    add("delete", "Excluir…", "trash");
    this.menu.show(trigger, x, y);
  }
  private async action(action: string) {
    const s = this.session;
    if (!s || s.state !== "connected") return;
    try {
      if (action === "up") {
        const parent = this.parents.get(s.id);
        if (parent) await this.refresh(s.id, parent.id);
        return;
      }
      if (action === "refresh") {
        await this.refresh();
        return;
      }
      const target = this.target();
      const entry = this.selected?.entry;
      if (action === "terminal" && entry?.kind === "directory") {
        this.terminal?.(s.id, entry);
        return;
      }
      if (action === "open" && entry?.kind === "file") {
        this.open(s.id, entry);
        return;
      }
      if (action === "upload" && target) {
        this.upload(s.id, target.directory);
        return;
      }
      if (action === "download" && entry) {
        this.download(s.id, entry);
        return;
      }
      if ((action === "file" || action === "folder") && target) {
        const name = await askName(
          action === "file" ? "Novo arquivo" : "Nova pasta",
          `Destino remoto: ${target.directory.path}`,
        );
        if (!name) return;
        await api.mutateFiles(this.generation, s.id, {
          action: "create",
          parent: target.directory.id,
          name,
          directory: action === "folder",
        });
      } else if ((action === "rename" || action === "delete") && entry) {
        const name =
          action === "rename"
            ? await askName("Renomear", entry.path, entry.name)
            : null;
        if (action === "rename" && !name) return;
        const result = await api.mutateFiles(this.generation, s.id, {
          action: "prepare",
          entry: entry.id,
          name,
        });
        if (result.kind !== "confirmation") return;
        const decision = await choose(
          action === "delete"
            ? "Excluir entrada remota"
            : "Confirmar renomeação",
          `${result.entry.path}${result.target ? ` → ${result.target}` : "\nPastas só são removidas se estiverem vazias. Links são removidos sem seguir o destino."}`,
          [
            { label: "Cancelar", value: "cancel" },
            { label: "Confirmar", value: "apply", danger: action === "delete" },
          ],
        );
        if (decision !== "apply") return;
        await api.mutateFiles(this.generation, s.id, {
          action: "apply",
          token: result.token,
        });
      } else return;
      this.report("Operação remota concluída.");
      await this.refresh(s.id);
    } catch (error) {
      this.report(safeError(error));
    }
  }
  private async release(session: string, node: Node): Promise<void> {
    if (node.cursor)
      await api
        .releaseFiles(this.generation, session, undefined, node.cursor)
        .catch(() => {});
    await Promise.all(node.children.map((n) => this.release(session, n)));
  }
  closeSession(session: string) {
    const root = this.trees.get(session);
    if (root) void this.release(session, root);
    this.trees.delete(session);
    this.parents.delete(session);
  }
  dispose() {
    this.alive = false;
    this.menu.dispose();
    for (const [session, node] of this.trees) void this.release(session, node);
    this.trees.clear();
  }
}
