import { brand } from "../ui/brand";
import { ConnectionPicker } from "../features/connections/picker";
import { icon } from "../ui/icons";
import { DockerPanel } from "../features/docker/view";
import {
  draftsList,
  draftRemove,
  draftRead,
  type DraftInfo,
} from "../ipc/lifecycle";
import { lockMinutes } from "./lifecycle";
import { FileBrowser } from "../features/files/browser";
import type { EditorPool } from "../features/editor/live";
import { TransferPanel } from "../features/transfers/view";
import { choose } from "../features/files/dialogs";
import * as api from "../ipc/connections";
import { safeError } from "../ipc/client";
import type { Profile, SessionInfo, CredentialInput } from "../ipc/contracts";
import {
  editProfile,
  transientCredential,
  removeProfile,
  hostConfirmation,
} from "../features/connections/dialogs";
import { readLayout, saveLayout, bindDivider, defaults } from "./layout";
import type { TerminalPool } from "../features/terminal/live";
const states: Record<SessionInfo["state"], string> = {
  connecting: "Conectando",
  host_pending: "Confirmar identidade",
  connected: "Conectado",
  disconnected: "Desconectado",
  error: "Erro",
};
export function mountWorkspace(
  root: HTMLElement,
  generation: number,
  onVault: () => void,
  onLock: () => void,
) {
  let alive = true,
    profiles: Profile[] = [],
    sessions: SessionInfo[] = [],
    selected: string | undefined,
    active: string | undefined,
    polling = false;
  let pool: TerminalPool | undefined;
  const automatic = new Set<string>(),
    challenges = new Set<string>();
  let currentView = "overview";
  let sidebarMode: "files" | "connections" = "connections";
  root.innerHTML = `<div class="workspace live-workspace">
    <header class="topbar" data-tauri-drag-region><div class="connection-picker"><button id="connection-switcher" class="connection-switcher" aria-haspopup="menu" aria-expanded="false" aria-controls="connection-popover">${brand("mark", true)}<span id="active-name">Conexões</span></button><div id="connection-popover" role="menu" aria-label="Selecionar conexão" hidden></div></div><span class="server-badge" id="active-address"></span><label class="idle-setting" title="Bloquear após inatividade">${icon("lock")}<select id="lock-minutes" aria-label="Minutos de inatividade"><option>1</option><option>5</option><option>15</option><option>30</option><option>60</option><option>120</option><option>240</option></select><span>min</span></label><button id="vault-menu" title="Trocar senha mestra" aria-label="Configurações do cofre">${icon("settings")}</button><button id="lock" title="Bloquear cofre">${icon("lock")}<span>Bloquear</span></button></header>
    <nav class="rail" aria-label="Ferramentas"><button id="connections-tool" aria-label="Mostrar ou ocultar conexões" title="Conexões">${icon("folder")}</button><button id="overview-tool" aria-label="Visão geral" title="Visão geral">${icon("home")}</button><button id="docker-tool" aria-label="Docker" title="Docker">${icon("docker")}</button><div class="rail-bottom"><button id="terminal-tool" aria-label="Mostrar ou ocultar terminal" title="Terminal">${icon("terminal")}</button><button id="restore" aria-label="Restaurar layout" title="Restaurar layout">${icon("restore")}</button></div></nav>
    <aside class="sidebar island" aria-label="Conexões e arquivos"><section id="connections-panel" class="side-panel"><div class="panel-heading"><h2>Conexões</h2><span class="count" id="profile-count">0</span><button id="show-files" aria-label="Voltar aos arquivos" title="Voltar aos arquivos">${icon("folder")}</button><button id="new-profile" aria-label="Nova conexão" title="Nova conexão">${icon("plus")}</button></div><label class="search">${icon("search")}<input id="search" placeholder="Pesquisar conexões" aria-label="Pesquisar conexões"></label><div class="profile-actions"><button id="edit-profile" disabled>Editar</button><button id="duplicate-profile" disabled>Duplicar</button><button id="remove-profile" disabled>Excluir</button></div><div id="connection-list" role="group" aria-label="Conexões salvas"></div><p id="profiles-empty" class="empty" role="status">Carregando conexões…</p></section><section id="files-panel" class="side-panel" hidden><div class="files-heading"><h2>Arquivos</h2><span class="context-label" id="files-context">Sem sessão</span><button id="show-connections" aria-label="Mostrar conexões salvas" title="Conexões salvas">${icon("connection")}</button></div><div id="file-browser"></div></section></aside>
    <div id="sidebar-divider" class="divider vertical" role="separator" tabindex="0" aria-label="Largura do painel lateral" aria-orientation="vertical" aria-valuemin="190" aria-valuemax="420"></div>
    <main class="center"><section class="editor-island island" aria-label="Área de trabalho"><div class="editor-tabs"><div id="document-tabs"></div><div class="workspace-tools"><button id="overview-tab" class="tab-active" title="Início" aria-label="Início">${icon("home")}</button><button id="docker-tab">Docker</button><button id="drafts-tab">Rascunhos</button></div><span class="context-label" id="editor-context" hidden>Sem sessão</span></div><div class="session-overview" id="overview"><div class="welcome-brand">${brand("compact")}</div><h1 id="session-title">Abra uma conexão.</h1><p class="muted" id="session-detail">Cadastre um servidor e abra uma sessão SSH para trabalhar no terminal.</p><p id="session-error" role="alert"></p><div class="overview-actions"><button class="primary" id="connect-selected">Nova conexão</button><button id="disconnect" disabled>Desconectar</button></div></div><div id="documents-view" hidden><div id="document-editor"></div></div><div id="transfer-panel" hidden></div><div id="drafts-view" class="session-overview" hidden><p class="eyebrow">TRABALHO PROTEGIDO</p><h1>Rascunhos recuperáveis.</h1><p class="muted">Conecte o perfil original para recuperar em uma aba. Nada é gravado no servidor automaticamente.</p><div id="draft-list"></div></div><div id="docker-view" class="docker-live" hidden></div></section></main>
    <div id="terminal-divider" class="divider horizontal" role="separator" tabindex="0" aria-label="Altura do terminal" aria-orientation="horizontal" aria-valuemin="140" aria-valuemax="460"></div>
    <section class="terminal-island island" aria-label="Terminal"><div class="panel-heading"><h2>Terminal <span class="muted" id="terminal-context">/ sem sessão</span></h2><div class="terminal-actions"><button id="new-terminal" disabled>+ Terminal</button><button id="copy-terminal">Copiar</button><button id="paste-terminal" title="Colar no servidor ativo">Colar</button><button id="interrupt-terminal" title="Enviar Ctrl+C ao servidor ativo">Ctrl+C</button><button id="hide-terminal" aria-label="Recolher terminal">−</button></div></div><div id="terminal-tabs" class="terminal-tabs" role="group" aria-label="Abas de terminal"></div><div id="terminal-surface" class="terminal-surface terminal-empty"></div></section>
    <footer class="statusbar"><span id="session-status">Sem conexão SSH</span><span id="notice" role="status">Cofre local protegido</span><span>SSH / UTF-8 / unny-ssh</span></footer></div>`;
  const element = <T extends HTMLElement>(selector: string) =>
    root.querySelector<T>(selector)!;
  const report = (message: string) => {
    if (alive) element("#notice").textContent = message;
  };
  const invoke = async (action: () => Promise<unknown>) => {
    try {
      await action();
    } catch (error) {
      report(safeError(error));
    }
  };
  const current = () => sessions.find((s) => s.id === active);
  const docker = new DockerPanel(
    generation,
    element("#docker-view"),
    report,
    (session, resource) => {
      layout.terminalHidden = false;
      apply();
      void pool?.open(session, resource);
    },
  );
  const transfers = new TransferPanel(
    generation,
    element("#transfer-panel"),
    report,
    (s) => void browser.refresh(s),
    () => browser.target(),
  );
  let editor: EditorPool | undefined;
  const editorReady = import("../features/editor/live")
    .then((module) => {
      if (!alive) return;
      editor = new module.EditorPool(
        generation,
        element("#document-tabs"),
        element("#document-editor"),
        report,
        () => view("documents"),
        (s, e) => void transfers.download(s, e),
      );
      editor?.show(active);
      return editor;
    })
    .catch((error) => {
      report(safeError(error));
      return undefined;
    });
  const browser = new FileBrowser(
    generation,
    element("#file-browser"),
    (s, e) => void editorReady.then((pool) => pool?.open(s, e)),
    (s, e) => void transfers.download(s, e),
    (s, e) => void transfers.pickUploads(s, e),
    report,
    (session, entry) => {
      layout.terminalHidden = false;
      apply();
      if (pool) void pool.open(session, undefined, entry);
      else report("Aguarde o terminal carregar.");
    },
  );
  const guard = async (session?: string, allowDrafts = false) => {
    if (transfers.hasActive(session)) {
      if (
        (await choose(
          "Transferências em andamento",
          "Continuar cancela as transferências deste contexto. Arquivos parciais serão identificados como incompletos.",
          [
            { label: "Cancelar saída", value: "stay" },
            {
              label: "Cancelar transferências e continuar",
              value: "leave",
              danger: true,
            },
          ],
        )) !== "leave"
      )
        return false;
    }
    if (editor && !(await editor.guard(session, allowDrafts))) return false;
    await transfers.cancelAll(session);
    return true;
  };

  let drafts: DraftInfo[] = [];
  const drawDrafts = () => {
    const list = element("#draft-list");
    list.replaceChildren();
    element("#drafts-tab").textContent = `Rascunhos (${drafts.length})`;
    for (const draft of drafts) {
      const row = document.createElement("article");
      row.className = "draft-card";
      const title = document.createElement("strong"),
        path = document.createElement("p"),
        restore = document.createElement("button"),
        remove = document.createElement("button");
      title.textContent = draft.name;
      path.textContent = `${draft.server} · ${draft.path}`;
      restore.textContent = "Recuperar na sessão ativa";
      restore.disabled =
        current()?.state !== "connected" ||
        current()?.profile.id !== draft.profileId;
      restore.onclick = () => {
        if (active)
          void editorReady.then((pool) => pool?.restore(active!, draft.id));
      };
      remove.textContent = "Excluir rascunho";
      remove.onclick = () =>
        void invoke(async () => {
          if (
            (await choose(
              "Excluir rascunho protegido",
              `${draft.path}\nO arquivo remoto não será alterado.`,
              [
                { label: "Cancelar", value: "cancel" },
                { label: "Excluir rascunho", value: "remove", danger: true },
              ],
            )) === "remove"
          ) {
            await draftRemove(generation, draft.id);
            drafts = await draftsList(generation);
            if (alive) drawDrafts();
          }
        });
      const preview = document.createElement("button");
      preview.textContent = "Ver conteúdo";
      preview.onclick = () =>
        void invoke(async () => {
          const data = await draftRead(generation, draft.id);
          if (!alive) return;
          const modal = document.createElement("dialog");
          modal.className = "connection-dialog";
          const heading = document.createElement("h1"),
            body = document.createElement("textarea"),
            close = document.createElement("button");
          heading.textContent = draft.name;
          body.value = data.text;
          body.readOnly = true;
          body.className = "draft-preview";
          body.setAttribute("aria-label", "Conteúdo do rascunho");
          close.textContent = "Fechar";
          close.onclick = () => modal.close();
          modal.onclose = () => {
            body.value = "";
            modal.remove();
          };
          modal.append(heading, body, close);
          root.append(modal);
          modal.showModal();
        });
      row.append(title, path, preview, restore, remove);
      list.append(row);
    }
    if (!drafts.length) list.textContent = "Nenhum rascunho pendente.";
  };
  void draftsList(generation)
    .then((items) => {
      if (alive) {
        drafts = items;
        drawDrafts();
        if (items.length) view("drafts");
      }
    })
    .catch((error) => report(safeError(error)));
  element<HTMLSelectElement>("#lock-minutes").value = String(lockMinutes());
  element<HTMLSelectElement>("#lock-minutes").onchange = () => {
    localStorage.setItem(
      "unny.lock-minutes",
      element<HTMLSelectElement>("#lock-minutes").value,
    );
  };
  const profile = () => profiles.find((p) => p.id === selected);
  const layout = readLayout();
  const workspace = element(".workspace");
  workspace.classList.toggle("macos", navigator.userAgent.includes("Mac"));
  const apply = () => {
    workspace.style.setProperty("--sidebar", `${layout.sidebar}px`);
    workspace.style.setProperty(
      "--terminal",
      `${Math.min(layout.terminal, Math.max(140, window.innerHeight - 390))}px`,
    );
    workspace.classList.toggle("sidebar-hidden", layout.sidebarHidden);
    workspace.classList.toggle("terminal-hidden", layout.terminalHidden);
    element("#sidebar-divider").setAttribute(
      "aria-valuenow",
      String(layout.sidebar),
    );
    element("#terminal-divider").setAttribute(
      "aria-valuenow",
      String(layout.terminal),
    );
    saveLayout(layout);
  };
  apply();
  bindDivider(
    element("#sidebar-divider"),
    "x",
    () => layout.sidebar,
    (n) => {
      layout.sidebar = Math.min(420, Math.max(190, n));
      apply();
    },
  );
  bindDivider(
    element("#terminal-divider"),
    "y",
    () => layout.terminal,
    (n) => {
      layout.terminal = Math.min(460, Math.max(140, n));
      apply();
    },
  );
  const toggleSide = () => {
    layout.sidebarHidden = !layout.sidebarHidden;
    apply();
  };
  const toggleTerminal = () => {
    layout.terminalHidden = !layout.terminalHidden;
    apply();
  };
  element<HTMLButtonElement>("#connections-tool").onclick = toggleSide;
  element<HTMLButtonElement>("#terminal-tool").onclick = toggleTerminal;
  element<HTMLButtonElement>("#hide-terminal").onclick = () => {
    toggleTerminal();
    element("#terminal-tool").focus();
  };
  element<HTMLButtonElement>("#restore").onclick = () => {
    Object.assign(layout, defaults);
    apply();
  };
  element<HTMLButtonElement>("#vault-menu").onclick = onVault;
  element<HTMLButtonElement>("#lock").onclick = onLock;
  const refreshProfiles = async () => {
    const result = await api.profilesList(generation);
    if (!alive) return;
    profiles = result;
    if (!selected || !profiles.some((p) => p.id === selected))
      selected = profiles[0]?.id ?? undefined;
    drawProfiles();
    drawContext();
  };
  const newProfile = () =>
    editProfile(generation, undefined, () => void invoke(refreshProfiles));
  element<HTMLButtonElement>("#new-profile").onclick = newProfile;
  element<HTMLButtonElement>("#edit-profile").onclick = () => {
    if (profile())
      editProfile(generation, profile(), () => void invoke(refreshProfiles));
  };
  element<HTMLButtonElement>("#duplicate-profile").onclick = () =>
    void invoke(async () => {
      const p = profile();
      if (p) {
        const copy = await api.profileDuplicate(generation, p.id!);
        selected = copy.id!;
        report("Conexão duplicada sem copiar a credencial.");
        await refreshProfiles();
      }
    });
  element<HTMLButtonElement>("#remove-profile").onclick = () =>
    void invoke(async () => {
      const p = profile();
      if (p)
        await removeProfile(generation, p, () => void invoke(refreshProfiles));
    });
  const connect = async (p: Profile, credential?: CredentialInput) => {
    const session = await api.sessionConnect(generation, p.id!, credential);
    if (!alive) return;
    active = session.id;
    sidebarMode = "files";
    layout.sidebarHidden = false;
    apply();
    sessions.push(session);
    drawSessions();
    drawContext();
  };
  const connectProfile = (p: Profile) => {
    selected = p.id!;
    if (p.authMode !== "agent" && !p.hasSavedSecret)
      transientCredential(generation, p, (credential) =>
        connect(p, credential),
      );
    else void invoke(() => connect(p));
  };
  function drawProfiles() {
    const list = element("#connection-list");
    list.replaceChildren();
    const query =
      element<HTMLInputElement>("#search").value.toLocaleLowerCase();
    let count = 0;
    for (const p of profiles) {
      if (
        !`${p.name} ${p.host} ${p.username}`.toLocaleLowerCase().includes(query)
      )
        continue;
      count++;
      const row = document.createElement("div");
      row.className = `profile-row ${p.id === selected ? "selected" : ""}`;
      row.dataset.profileId = p.id!;
      const select = document.createElement("button");
      select.className = "profile-select";
      select.setAttribute("aria-pressed", String(p.id === selected));
      const name = document.createElement("strong"),
        address = document.createElement("small"),
        state = document.createElement("small");
      name.textContent = p.name;
      address.textContent = `${p.username}@${p.host}:${p.port}`;
      const related = sessions.filter((s) => s.profile.id === p.id);
      const live =
        related.find((s) => s.state === "connected") ??
        related.find(
          (s) => s.state === "host_pending" || s.state === "connecting",
        ) ??
        related.find((s) => s.state === "error");
      state.textContent = live ? states[live.state] : "Desconectado";
      select.append(name, address, state);
      select.onclick = () => {
        selected = p.id!;
        for (const item of list.querySelectorAll<HTMLElement>(".profile-row")) {
          const chosen = item.dataset.profileId === selected;
          item.classList.toggle("selected", chosen);
          item
            .querySelector(".profile-select")
            ?.setAttribute("aria-pressed", String(chosen));
        }
        for (const id of [
          "edit-profile",
          "duplicate-profile",
          "remove-profile",
        ])
          element<HTMLButtonElement>(`#${id}`).disabled = false;
        drawContext();
      };
      select.ondblclick = () => connectProfile(p);
      const connectButton = document.createElement("button");
      connectButton.textContent = "↗";
      connectButton.setAttribute("aria-label", `Conectar a ${p.name}`);
      connectButton.onclick = () => connectProfile(p);
      row.append(select, connectButton);
      list.append(row);
    }
    element("#profile-count").textContent = String(profiles.length);
    const empty = element("#profiles-empty");
    empty.hidden = count > 0;
    empty.textContent = profiles.length
      ? "Nenhuma conexão encontrada."
      : "Nenhuma conexão salva. Use + para cadastrar seu primeiro servidor.";
    for (const id of ["edit-profile", "duplicate-profile", "remove-profile"])
      element<HTMLButtonElement>(`#${id}`).disabled = !profile();
  }
  element<HTMLInputElement>("#search").oninput = drawProfiles;
  function closeSession(id: string) {
    void invoke(async () => {
      const session = sessions.find((s) => s.id === id);
      if (!session || !(await guard(session.id))) return;
      docker.closeSession(session.id);
      editor?.closeSession(session.id);
      browser.closeSession(session.id);
      await api.closeSession(generation, session.id);
      pool?.closeSession(session.id);
      sessions = sessions.filter((s) => s.id !== session.id);
      if (active === session.id) active = sessions[0]?.id;
      drawSessions();
      drawProfiles();
      drawContext();
    });
  }
  function drawSessions() {
    picker.update(profiles, sessions, active);
  }
  function drawContext() {
    const session = current();
    if (!session && layout.sidebarHidden) {
      layout.sidebarHidden = false;
      apply();
    }
    const name = session?.profile.name ?? "Nenhuma sessão";
    const address = session
      ? `${session.profile.username}@${session.profile.host}:${session.profile.port}`
      : "";
    picker.update(profiles, sessions, active);
    element("#connections-panel").hidden =
      !!session && sidebarMode !== "connections";
    element("#files-panel").hidden = !session || sidebarMode !== "files";
    element("#show-files").hidden = !session;
    element("#active-address").textContent = address;
    element("#files-context").textContent = name;
    element("#editor-context").textContent = name;
    element("#terminal-context").textContent = `/ ${name}`;

    element("#session-status").textContent = session
      ? `${session.profile.name} · ${states[session.state]}`
      : "Sem conexão SSH";
    element("#session-title").textContent = session
      ? `${states[session.state]} · ${session.profile.name}`
      : "Abra uma conexão.";
    element("#session-detail").textContent = session
      ? `${address}${session.profile.initialDirectory ? ` · ${session.profile.initialDirectory}` : ""}`
      : "Escolha uma conexão salva na lateral ou adicione um servidor para começar.";
    element("#session-error").textContent = session?.errorMessage ?? "";
    drawDrafts();
    docker.show(session, currentView === "docker");
    browser.show(session);
    editor?.show(active);
    transfers.show(active);
    const connectButton = element<HTMLButtonElement>("#connect-selected");
    connectButton.textContent =
      session?.state === "connected"
        ? "Explorar arquivos"
        : profile()
          ? `Conectar a ${profile()!.name}`
          : "Nova conexão";
    connectButton.onclick = () => {
      if (current()?.state === "connected") {
        sidebarMode = "files";
        layout.sidebarHidden = false;
        apply();
        drawContext();
        element(".file-viewport").focus();
        return;
      }
      const p = profile();
      if (p) connectProfile(p);
      else newProfile();
    };
    element<HTMLButtonElement>("#disconnect").disabled =
      !session ||
      !["connected", "connecting", "host_pending"].includes(session.state);
    element<HTMLButtonElement>("#new-terminal").disabled =
      session?.state !== "connected" || !pool;
    pool?.show(active);
  }
  element<HTMLButtonElement>("#disconnect").onclick = () =>
    void invoke(async () => {
      if (active) {
        await api.disconnect(generation, active);
        await poll();
      }
    });
  element<HTMLButtonElement>("#new-terminal").onclick = () => {
    if (active && pool) void pool.open(active);
  };
  element<HTMLButtonElement>("#copy-terminal").onclick = () => pool?.copy();
  element<HTMLButtonElement>("#paste-terminal").onclick = () =>
    void pool?.paste();
  element<HTMLButtonElement>("#interrupt-terminal").onclick = () =>
    pool?.interrupt();
  const view = (name: string) => {
    currentView = name;
    docker.show(current(), name === "docker");
    element("#overview").hidden = name !== "overview";
    element("#docker-view").hidden = name !== "docker";
    element("#documents-view").hidden = name !== "documents";
    element("#drafts-view").hidden = name !== "drafts";
    element("#drafts-tab").classList.toggle("tab-active", name === "drafts");
    element("#overview-tab").classList.toggle(
      "tab-active",
      name === "overview",
    );
    element("#docker-tab").classList.toggle("tab-active", name === "docker");
  };
  for (const id of ["overview-tool", "overview-tab"])
    element<HTMLButtonElement>(`#${id}`).onclick = () => view("overview");
  for (const id of ["docker-tool", "docker-tab"])
    element<HTMLButtonElement>(`#${id}`).onclick = () => view("docker");
  const picker = new ConnectionPicker(
    element<HTMLButtonElement>("#connection-switcher"),
    element("#connection-popover"),
    {
      create: newProfile,
      manage: () => {
        sidebarMode = "connections";
        layout.sidebarHidden = false;
        apply();
        drawContext();
        element<HTMLInputElement>("#search").focus();
      },
      select: (id) => {
        active = id;
        sidebarMode = "files";
        layout.sidebarHidden = false;
        apply();
        drawContext();
      },
      close: closeSession,
      connect: (p, newSession) => {
        const existing =
          !newSession &&
          sessions.find(
            (s) =>
              s.profile.id === p.id &&
              ["connected", "connecting", "host_pending"].includes(s.state),
          );
        if (existing) {
          active = existing.id;
          sidebarMode = "files";
          drawContext();
        } else connectProfile(p);
      },
    },
  );
  element<HTMLButtonElement>("#show-connections").onclick = () => {
    sidebarMode = "connections";
    drawContext();
  };
  element<HTMLButtonElement>("#show-files").onclick = () => {
    sidebarMode = "files";
    drawContext();
  };
  element<HTMLButtonElement>("#drafts-tab").onclick = () => view("drafts");
  view(currentView);
  let signature = "";
  async function poll() {
    if (!alive || polling) return;
    polling = true;
    try {
      const result = await api.sessionsList(generation);
      if (!alive) return;
      sessions = result;
      const next = JSON.stringify(sessions);
      if (signature !== next) {
        signature = next;
        if (active && !sessions.some((s) => s.id === active))
          active = sessions[0]?.id;
        drawSessions();
        drawProfiles();
        drawContext();
      }
      for (const session of sessions) {
        if (
          session.state === "connected" &&
          pool &&
          !automatic.has(session.id)
        ) {
          automatic.add(session.id);
          void pool.open(session.id);
        }
        if (session.challenge && !challenges.has(session.challenge.id)) {
          challenges.add(session.challenge.id);
          hostConfirmation(generation, session, () => void poll());
        }
      }
      pool?.refresh();
    } catch (error) {
      report(safeError(error));
    } finally {
      polling = false;
    }
  }
  void import("../features/terminal/live")
    .then((module) => {
      if (!alive) return;
      pool = new module.TerminalPool(
        generation,
        element("#terminal-tabs"),
        element("#terminal-surface"),
        report,
      );
      pool.show(active);
      drawContext();
      void poll();
    })
    .catch((error) => report(safeError(error)));
  void invoke(refreshProfiles);
  void poll();
  const timer = setInterval(() => void poll(), 1000);
  const shortcut = (event: KeyboardEvent) => {
    if (
      !event.metaKey &&
      event.target instanceof Element &&
      event.target.closest(".xterm")
    )
      return;
    if ((event.metaKey || event.ctrlKey) && event.key === "b") {
      event.preventDefault();
      toggleSide();
    }
    if ((event.metaKey || event.ctrlKey) && event.key === "j") {
      event.preventDefault();
      toggleTerminal();
    }
  };
  window.addEventListener("keydown", shortcut);
  window.addEventListener("resize", apply);
  const dispose = () => {
    alive = false;
    picker.dispose();
    docker.dispose();
    browser.dispose();
    editor?.dispose();
    transfers.dispose();
    clearInterval(timer);
    window.removeEventListener("keydown", shortcut);
    window.removeEventListener("resize", apply);
    root.querySelectorAll("dialog").forEach((d) => {
      for (const input of d.querySelectorAll<HTMLInputElement>(
        'input[type="password"]',
      ))
        input.value = "";
      if (d.open) d.close();
      d.remove();
    });
    pool?.dispose();
  };
  return Object.assign(dispose, {
    beforeLeave: () => guard(),
    beforeExit: () => guard(undefined, true),
    capture: async () => {
      await editorReady;
      return editor?.capture() ?? [];
    },
  });
}
