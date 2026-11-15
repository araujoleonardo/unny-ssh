import { brand } from "../ui/brand";
import { LockCoordinator, IdleLock, lockMinutes } from "./lifecycle";
import { workspaceLock, recoverLock } from "../ipc/lifecycle";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { finishWindowClose } from "../ipc/files";
import { mountWorkspace } from "./workspace";
import { native, safeError, vaultStatus } from "../ipc/client";
import type { VaultStatus } from "../ipc/contracts";
import { vaultForm, type VaultMode } from "../features/vault/view";
import { bindDivider, defaults, readLayout, saveLayout } from "./layout";
const root = document.querySelector<HTMLDivElement>("#app")!;
let status: VaultStatus = { state: "locked", generation: 0 };
let cleanup = () => {};
let beforeLeave = async () => true;
let renderGeneration = 0;
let beforeExit = async () => true;
let eventsInstalled = false;
let shield: HTMLElement | undefined;
const restoreWorkspace = () => {
  shield?.remove();
  shield = undefined;
  root.style.visibility = "";
  root.inert = false;
};
const coordinator = new LockCoordinator({
  hide: () => {
    root.style.visibility = "hidden";
    root.inert = true;
    shield = document.createElement("main");
    shield.className = "vault-screen lock-shield";
    shield.innerHTML =
      '<section class="vault-card"><p class="eyebrow">PROTEGENDO O AMBIENTE</p><h1>Guardando seu trabalho.</h1><p role="status">Protegendo rascunhos e encerrando sessões. Transferências são interrompidas; comandos já enviados podem ter produzido efeitos.</p></section>';
    document.body.append(shield);
  },
  restore: restoreWorkspace,
  persist: (buffers) => workspaceLock(status.generation, buffers),
  finish: (next) => {
    showStatus(next);
    restoreWorkspace();
    if (next.state !== "unlocked")
      root.querySelector<HTMLInputElement>("input")?.focus();
  },
  failed: (error) => {
    const notice = root.querySelector<HTMLElement>("#notice");
    if (notice)
      notice.textContent = `O bloqueio não foi concluído: ${safeError(error)}`;
  },
});
const idle = new IdleLock(
  () => Date.now(),
  lockMinutes,
  () => {
    if (status.state === "unlocked") void lock();
  },
);
const demos = ["Sandbox", "Staging"];
let active = 0;
let workspaceView: "editor" | "docker" = "editor";

function showStatus(next: VaultStatus) {
  if (next.generation < status.generation) return;
  status = next;
  idle.reset();
  render();
}
function openVaultDialog(mode: VaultMode) {
  const returnFocus = root.querySelector<HTMLElement>(
    mode === "reset" ? ".text-button" : "#vault-menu",
  );
  const dialog = document.createElement("dialog");
  dialog.setAttribute(
    "aria-label",
    mode === "reset" ? "Redefinir cofre" : "Trocar senha mestra",
  );
  dialog.innerHTML =
    '<button class="dialog-close" aria-label="Fechar diálogo">×</button><div class="dialog-content"></div>';
  vaultForm(dialog.querySelector(".dialog-content")!, mode, (next) => {
    dialog.close();
    showStatus(next);
  });
  dialog.querySelector("button")!.onclick = () => dialog.close();
  dialog.onclose = () => {
    dialog.remove();
    if (returnFocus?.isConnected) returnFocus.focus();
  };
  root.append(dialog);
  dialog.showModal();
}
async function lock() {
  if (native && status.state === "unlocked") await coordinator.lock();
}
function failedLockForm(container: HTMLElement) {
  container.innerHTML =
    '<p class="eyebrow">BLOQUEIO INCOMPLETO</p><h1>Seu trabalho continua na memória.</h1><p class="muted">Não foi possível proteger os rascunhos no disco. Sessões foram encerradas, mas a chave permanece na memória. Confira espaço e permissões. Autentique para tentar novamente ou descartar somente os rascunhos pendentes. Não encerre o processo antes de decidir.</p><form class="vault-form"><label>Senha mestra<input type="password" name="password" required autocomplete="current-password" maxlength="1024"></label><p class="form-error" role="alert"></p><button type="submit" class="primary">Tentar proteger e reabrir</button><button type="button" class="danger">Descartar pendentes e bloquear</button></form>';
  const form = container.querySelector("form")!,
    input = form.querySelector("input")!,
    error = form.querySelector<HTMLElement>(".form-error")!;
  const recover = async (discard: boolean) => {
    const password = input.value;
    input.value = "";
    form.querySelectorAll("button").forEach((b) => (b.disabled = true));
    try {
      showStatus(await recoverLock(password, discard));
    } catch (e) {
      error.textContent = safeError(e);
    } finally {
      form.querySelectorAll("button").forEach((b) => (b.disabled = false));
    }
  };
  form.onsubmit = (e) => {
    e.preventDefault();
    void recover(false);
  };
  form.querySelector<HTMLButtonElement>('[type="button"]')!.onclick = () => {
    if (input.reportValidity()) void recover(true);
  };
  input.focus();
}
function render() {
  cleanup();
  cleanup = () => {};
  beforeLeave = async () => true;
  beforeExit = async () => true;
  renderGeneration++;
  if (status.state !== "unlocked" && native) {
    root.innerHTML =
      '<main class="vault-screen"><section class="vault-card"></section><footer>unny-ssh <span>Seu próximo ambiente começa aqui.</span></footer></main>';
    const card = root.querySelector<HTMLElement>(".vault-card")!;
    if (status.state === "lock_failed") {
      failedLockForm(card);
      return;
    }
    vaultForm(
      card,
      status.state === "missing" ? "create" : "unlock",
      showStatus,
    );
    if (status.state === "locked") {
      const reset = document.createElement("button");
      reset.className = "text-button";
      reset.textContent = "Esqueci minha senha / redefinir cofre";
      reset.onclick = () => openVaultDialog("reset");
      card.append(reset);
    }
    card.querySelector("input")?.focus();
    return;
  }
  if (native) {
    const workspace = mountWorkspace(
      root,
      status.generation,
      () =>
        void beforeLeave().then((ok) => {
          if (ok) openVaultDialog("change_password");
        }),
      () => void lock(),
    );
    const unregister = coordinator.register(workspace);
    cleanup = () => {
      unregister();
      workspace();
    };
    beforeLeave = workspace.beforeLeave;
    beforeExit = workspace.beforeExit;
    return;
  }
  const context = demos[active];
  root.innerHTML = `<div class="workspace">
    <header class="topbar"><a class="wordmark" href="#" aria-label="unny ssh início">${brand("full", true)}</a><div class="breadcrumb">Workspace <span>/</span> <strong>${context}</strong></div><span class="demo-badge">DEMONSTRAÇÃO · SEM CONEXÃO SSH</span><button id="vault-menu">${native ? "Cofre desbloqueado" : "Prévia no navegador"}</button>${native ? '<button id="lock" title="Bloquear cofre">Bloquear ↗</button>' : ""}</header>
    <nav class="rail" aria-label="Ferramentas"><button id="connections-tool" aria-label="Mostrar ou ocultar conexões" title="Conexões">☷</button><button id="editor-tool" aria-label="Editor" title="Editor">⌘</button><button id="docker-tool" aria-label="Docker" title="Docker">▦</button><div class="rail-bottom"><button id="terminal-tool" aria-label="Mostrar ou ocultar terminal" title="Terminal">⌁</button><button id="restore" aria-label="Restaurar layout" title="Restaurar layout">↺</button></div></nav>
    <aside class="sidebar island" aria-label="Conexões e arquivos"><div class="panel-heading"><h2>Conexões</h2><span class="count">2</span></div><label class="search"><span aria-hidden="true">⌕</span><input id="search" placeholder="Pesquisar demonstrações" aria-label="Pesquisar conexões"></label><p class="section-label">AMBIENTES SIMULADOS</p><div id="connection-list">${demos.map((name, i) => `<button class="connection ${i === active ? "selected" : ""}" data-session="${i}" aria-pressed="${i === active}"><span class="server-icon" aria-hidden="true">▤</span><span><strong>${name}</strong><small>Linux · demonstração</small></span><span class="connection-state">Demo</span></button>`).join("")}</div><p id="no-results" class="empty" hidden>Nenhum ambiente encontrado.</p><div class="files-heading"><h2>Explorador</h2><span class="context-label">${context}</span></div><div class="file-tree"><p>⌄ &nbsp;workspace-demo</p><button class="file selected" id="sample-file">&nbsp;&nbsp; ⌘ &nbsp;welcome.sh <small>somente leitura</small></button></div><div class="sidebar-note"><span class="small-dot"></span> Dados de exemplo<br><small>Conexões de exemplo para explorar o layout.</small></div></aside>
    <div id="sidebar-divider" class="divider vertical" role="separator" tabindex="0" aria-label="Largura do painel lateral" aria-orientation="vertical" aria-valuemin="190" aria-valuemax="420"></div>
    <main class="center"><div class="sessions" role="group" aria-label="Sessão ativa">${demos.map((name, i) => `<button data-session="${i}" class="session ${i === active ? "active" : ""}" aria-pressed="${i === active}"><span class="small-dot"></span>${name}<span class="muted"> · demo</span></button>`).join("")}</div>
    <section class="editor-island island" aria-label="Área de trabalho"><div class="editor-tabs"><button id="editor-tab" class="${workspaceView === "editor" ? "tab-active" : ""}">⌘ &nbsp;welcome.sh</button><button id="docker-tab" class="${workspaceView === "docker" ? "tab-active" : ""}">▦ &nbsp;Docker</button><span class="context-label">${context} / demonstração</span></div><div id="editor" ${workspaceView !== "editor" ? "hidden" : ""} aria-label="Editor de demonstração"></div><div id="docker" ${workspaceView !== "docker" ? "hidden" : ""}><div class="docker-intro"><p class="eyebrow">${context.toUpperCase()} / DOCKER</p><h1>Infraestrutura, no mesmo lugar.</h1><p class="muted">Explore a estrutura do painel com dados de demonstração.</p></div><div class="docker-nav" role="group" aria-label="Inventário Docker">${["Containers", "Imagens", "Volumes", "Compose"].map((name, i) => `<button aria-pressed="${i === 0}">${name}</button>`).join("")}</div><div class="docker-empty"><span aria-hidden="true">▦</span><h2 id="docker-title">Containers</h2><p>Nenhum servidor conectado.</p><small>As ações remotas estarão disponíveis após conectar por SSH.</small></div></div></section>
    <div id="terminal-divider" class="divider horizontal" role="separator" tabindex="0" aria-label="Altura do terminal" aria-orientation="horizontal" aria-valuemin="140" aria-valuemax="460"></div>
    <section class="terminal-island island" aria-label="Terminal"><div class="panel-heading"><h2>Terminal <span class="muted">/ ${context}</span></h2><span class="demo-badge">SOMENTE LEITURA</span><button id="hide-terminal" aria-label="Recolher terminal">−</button></div><div id="terminal"></div></section></main>
    <footer class="statusbar"><span><span class="small-dot"></span> ${context} · simulado</span><span id="notice" role="status">${native ? "Cofre local protegido" : "Cofre disponível somente no aplicativo desktop"}</span><span>UTF-8 <span class="muted">/</span> Shell <span class="muted">/</span> unny-ssh</span></footer></div>`;
  const layout = readLayout();
  const workspace = root.querySelector<HTMLElement>(".workspace")!;
  const apply = () => {
    workspace.style.setProperty("--sidebar", `${layout.sidebar}px`);
    workspace.style.setProperty(
      "--terminal",
      `${Math.min(layout.terminal, Math.max(140, window.innerHeight - 390))}px`,
    );
    workspace.classList.toggle("sidebar-hidden", layout.sidebarHidden);
    workspace.classList.toggle("terminal-hidden", layout.terminalHidden);
    const side = root.querySelector<HTMLElement>("#sidebar-divider")!;
    const term = root.querySelector<HTMLElement>("#terminal-divider")!;
    side.setAttribute("aria-valuenow", String(layout.sidebar));
    term.setAttribute("aria-valuenow", String(layout.terminal));
    saveLayout(layout);
  };
  apply();
  bindDivider(
    root.querySelector("#sidebar-divider")!,
    "x",
    () => layout.sidebar,
    (n) => {
      layout.sidebar = Math.min(420, Math.max(190, n));
      apply();
    },
  );
  bindDivider(
    root.querySelector("#terminal-divider")!,
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
  root.querySelector<HTMLButtonElement>("#connections-tool")!.onclick =
    toggleSide;
  root.querySelector<HTMLButtonElement>("#terminal-tool")!.onclick =
    toggleTerminal;
  root.querySelector<HTMLButtonElement>("#hide-terminal")!.onclick = () => {
    toggleTerminal();
    root.querySelector<HTMLButtonElement>("#terminal-tool")!.focus();
  };
  root.querySelector<HTMLButtonElement>("#restore")!.onclick = () => {
    Object.assign(layout, defaults);
    apply();
  };
  root.querySelectorAll<HTMLButtonElement>("[data-session]").forEach(
    (button) =>
      (button.onclick = () => {
        active = Number(button.dataset.session);
        render();
        root
          .querySelector<HTMLButtonElement>(
            `.session[data-session="${active}"]`,
          )
          ?.focus();
      }),
  );
  const view = (name: "editor" | "docker") => {
    workspaceView = name;
    render();
    root
      .querySelector<HTMLButtonElement>(
        name === "editor" ? "#editor-tab" : "#docker-tab",
      )!
      .focus();
  };
  for (const id of ["editor-tool", "editor-tab", "sample-file"])
    root.querySelector<HTMLButtonElement>(`#${id}`)!.onclick = () =>
      view("editor");
  for (const id of ["docker-tool", "docker-tab"])
    root.querySelector<HTMLButtonElement>(`#${id}`)!.onclick = () =>
      view("docker");
  root.querySelectorAll<HTMLButtonElement>(".docker-nav button").forEach(
    (button) =>
      (button.onclick = () => {
        root
          .querySelectorAll(".docker-nav button")
          .forEach((el) =>
            el.setAttribute("aria-pressed", String(el === button)),
          );
        root.querySelector("#docker-title")!.textContent = button.textContent;
      }),
  );
  root.querySelector<HTMLInputElement>("#search")!.oninput = (event) => {
    const query = (event.target as HTMLInputElement).value.toLocaleLowerCase();
    let count = 0;
    root
      .querySelectorAll<HTMLButtonElement>(".connection")
      .forEach((button) => {
        button.hidden = !button
          .textContent!.toLocaleLowerCase()
          .includes(query);
        if (!button.hidden) count++;
      });
    root.querySelector<HTMLElement>("#no-results")!.hidden = count > 0;
  };
  if (native) {
    root.querySelector<HTMLButtonElement>("#lock")!.onclick = () => void lock();
    root.querySelector<HTMLButtonElement>("#vault-menu")!.onclick = () =>
      openVaultDialog("change_password");
  } else
    root.querySelector<HTMLButtonElement>("#vault-menu")!.onclick = () => {
      root.querySelector("#notice")!.textContent =
        "Execute npm run tauri dev para criar e acessar o cofre real.";
    };
  const token = renderGeneration;
  const disposers: (() => void)[] = [];
  if (workspaceView === "editor")
    void import("../features/editor/preview")
      .then((module) => {
        if (token === renderGeneration)
          disposers.push(
            module.mountEditor(root.querySelector("#editor")!, context),
          );
      })
      .catch(() => {
        if (token === renderGeneration)
          root.querySelector("#editor")!.textContent =
            "Não foi possível carregar o editor local. Reabra a área de trabalho.";
      });
  void import("../features/terminal/preview")
    .then((module) => {
      if (token === renderGeneration)
        disposers.push(
          module.mountTerminal(root.querySelector("#terminal")!, context),
        );
    })
    .catch(() => {
      if (token === renderGeneration)
        root.querySelector("#terminal")!.textContent =
          "Não foi possível carregar o terminal local.";
    });
  const shortcut = (event: KeyboardEvent) => {
    if (event.metaKey || event.ctrlKey) {
      if (event.key === "b") {
        event.preventDefault();
        toggleSide();
      }
      if (event.key === "j") {
        event.preventDefault();
        toggleTerminal();
      }
    }
  };
  window.addEventListener("keydown", shortcut);
  window.addEventListener("resize", apply);
  cleanup = () => {
    disposers.forEach((fn) => fn());
    window.removeEventListener("keydown", shortcut);
    window.removeEventListener("resize", apply);
  };
}
export async function start() {
  root.innerHTML =
    '<main class="vault-screen"><p role="status">Abrindo armazenamento protegido…</p></main>';
  if (!eventsInstalled) {
    eventsInstalled = true;
    for (const type of ["pointerdown", "keydown", "wheel", "touchstart"])
      window.addEventListener(type, () => {
        idle.activity();
      });
    window.addEventListener("keydown", (event) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.shiftKey &&
        event.key.toLowerCase() === "l"
      ) {
        event.preventDefault();
        if (status.state === "unlocked") void lock();
      }
    });
    setInterval(() => {
      if (native && status.state === "unlocked") idle.tick();
    }, 1000);
    document.addEventListener("visibilitychange", () => {
      if (document.hidden && status.state === "unlocked") void lock();
    });
    if (native)
      void listen("app-suspend", () => {
        if (status.state === "unlocked") void lock();
      });
    let leaving = false;
    const requestExit = async () => {
      if (leaving) return;
      leaving = true;
      try {
        if (status.state === "lock_failed") return;
        if (status.state === "unlocked") {
          if (!(await beforeExit())) return;
          await lock();
          if ((status as VaultStatus).state !== "locked") return;
        }
        await finishWindowClose();
      } finally {
        leaving = false;
      }
    };
    if (native) {
      void listen("app-close-requested", () => void requestExit());
      void getCurrentWindow().onCloseRequested((event) => {
        event.preventDefault();
        void requestExit();
      });
    }
  }
  if (!native) {
    render();
    return;
  }
  try {
    showStatus(await vaultStatus());
  } catch (e) {
    root.innerHTML =
      '<main class="vault-screen"><section class="vault-card"><h1>Armazenamento indisponível</h1><p role="alert"></p><button id="retry">Tentar novamente</button></section></main>';
    root.querySelector("p")!.textContent = safeError(e);
    root.querySelector<HTMLButtonElement>("#retry")!.onclick = () =>
      void start();
  }
}
