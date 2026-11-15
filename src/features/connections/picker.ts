import { brand } from "../../ui/brand";
import type { Profile, SessionInfo } from "../../ipc/contracts";
import { Popup } from "../../ui/popup";
import { icon } from "../../ui/icons";
const labels: Record<SessionInfo["state"], string> = {
  connected: "Conectado",
  connecting: "Conectando",
  host_pending: "Confirmar identidade",
  disconnected: "Desconectado",
  error: "Erro",
};
function badge(name: string) {
  const el = document.createElement("span");
  el.className = "connection-badge";
  el.textContent =
    name
      .trim()
      .split(/\s+/)
      .map((s) => s[0])
      .join("")
      .slice(0, 2)
      .toUpperCase() || "US";
  if (el.textContent.length === 1)
    el.textContent = name.slice(0, 2).toUpperCase();
  const palette = ["#5269e8", "#b87454", "#53867a", "#9a6bb4"];
  el.style.background =
    palette[
      [...name].reduce((n, c) => n + c.charCodeAt(0), 0) % palette.length
    ];
  el.setAttribute("aria-hidden", "true");
  return el;
}
export class ConnectionPicker {
  private popup: Popup;
  private profiles: Profile[] = [];
  private sessions: SessionInfo[] = [];
  private active?: string;
  private input: HTMLInputElement;
  private list: HTMLElement;
  constructor(
    private trigger: HTMLButtonElement,
    panel: HTMLElement,
    private actions: {
      create(): void;
      manage(): void;
      import?(): void;
      export?(): void;
      select(session: string): void;
      close(session: string): void;
      connect(profile: Profile, newSession: boolean): void;
    },
  ) {
    panel.innerHTML = `<div class="picker-actions"><button data-create role="menuitem">${icon("plus")}Nova conexão…</button><button data-manage role="menuitem">${icon("settings")}Gerenciar conexões…</button>${actions.import ? `<button data-import role="menuitem">${icon("upload")}Importar JSON…</button>` : ""}${actions.export ? `<button data-export role="menuitem">${icon("download")}Exportar JSON…</button>` : ""}</div><label class="picker-search">${icon("search")}<input placeholder="Pesquisar conexões" aria-label="Pesquisar no seletor de conexões"></label><div class="picker-list"></div>`;
    this.popup = new Popup(panel);
    this.input = panel.querySelector("input")!;
    this.list = panel.querySelector(".picker-list")!;
    panel.querySelector<HTMLButtonElement>("[data-create]")!.onclick = () => {
      this.popup.close();
      actions.create();
    };
    panel.querySelector<HTMLButtonElement>("[data-manage]")!.onclick = () => {
      this.popup.close();
      actions.manage();
    };
    for (const name of ["import", "export"] as const) {
      const button = panel.querySelector<HTMLButtonElement>(`[data-${name}]`);
      if (button)
        button.onclick = () => {
          this.popup.close();
          actions[name]?.();
        };
    }
    this.input.oninput = () => this.draw();
    trigger.onclick = () => this.popup.toggle(trigger);
    trigger.onkeydown = (e) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        this.popup.show(trigger);
      }
    };
  }
  update(profiles: Profile[], sessions: SessionInfo[], active?: string) {
    this.profiles = profiles;
    this.sessions = sessions;
    this.active = active;
    const current = sessions.find((s) => s.id === active);
    const name = current?.profile.name ?? "Conexões";
    if (current) this.trigger.replaceChildren(badge(current.profile.name));
    else this.trigger.innerHTML = brand("mark", true);
    const label = document.createElement("span");
    label.id = "active-name";
    label.textContent = name;
    const chevron = document.createElement("span");
    chevron.innerHTML = icon("chevron");
    chevron.className = "picker-chevron";
    this.trigger.append(label, chevron);
    this.trigger.title = current
      ? `${name} · ${labels[current.state]}`
      : "Selecionar conexão";
    this.draw();
  }
  private heading(text: string) {
    const label = document.createElement("p");
    label.className = "picker-heading";
    label.textContent = text;
    this.list.append(label);
  }
  private row(name: string, address: string, status?: string) {
    const row = document.createElement("div");
    row.className = "picker-row";
    const button = document.createElement("button");
    button.setAttribute("role", "menuitem");
    const content = document.createElement("span"),
      title = document.createElement("strong"),
      detail = document.createElement("small");
    title.textContent = name;
    detail.textContent = address;
    content.append(title, detail);
    if (status) {
      const state = document.createElement("small");
      state.className = "picker-state";
      state.textContent = status;
      content.append(state);
    }
    button.append(badge(name), content);
    row.append(button);
    return { row, button };
  }
  private draw() {
    this.list.replaceChildren();
    const query = this.input.value.toLocaleLowerCase();
    const matches = (name: string, address: string) =>
      `${name} ${address}`.toLocaleLowerCase().includes(query);
    const sessions = this.sessions.filter((s) =>
      matches(s.profile.name, `${s.profile.username}@${s.profile.host}`),
    );
    if (sessions.length) {
      this.heading("Sessões abertas");
      for (const s of sessions) {
        const { row, button } = this.row(
          s.profile.name,
          `${s.profile.username}@${s.profile.host}:${s.profile.port}`,
          labels[s.state],
        );
        button.classList.toggle("selected", s.id === this.active);
        button.onclick = () => {
          this.popup.close();
          this.actions.select(s.id);
        };
        const close = document.createElement("button");
        close.className = "picker-secondary";
        close.setAttribute("role", "menuitem");
        close.setAttribute("aria-label", `Fechar sessão ${s.profile.name}`);
        close.innerHTML = icon("close");
        close.onclick = () => {
          this.popup.close();
          this.actions.close(s.id);
        };
        row.append(close);
        this.list.append(row);
      }
    }
    this.heading("Conexões salvas");
    let count = 0;
    for (const p of this.profiles) {
      const address = `${p.username}@${p.host}:${p.port}`;
      if (!matches(p.name, address)) continue;
      count++;
      const { row, button } = this.row(p.name, address);
      button.onclick = () => {
        this.popup.close();
        this.actions.connect(p, false);
      };
      const add = document.createElement("button");
      add.className = "picker-secondary";
      add.setAttribute("role", "menuitem");
      add.setAttribute("aria-label", `Nova sessão de ${p.name}`);
      add.title = "Abrir outra sessão";
      add.innerHTML = icon("plus");
      add.onclick = () => {
        this.popup.close();
        this.actions.connect(p, true);
      };
      row.append(add);
      this.list.append(row);
    }
    if (!count) {
      const empty = document.createElement("p");
      empty.className = "picker-empty";
      empty.textContent = this.profiles.length
        ? "Nenhuma conexão encontrada."
        : "Nenhuma conexão salva.";
      this.list.append(empty);
    }
  }
  close() {
    this.popup.close(false);
  }
  dispose() {
    this.popup.dispose();
  }
}
