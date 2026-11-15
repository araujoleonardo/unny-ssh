import { Channel } from "@tauri-apps/api/core";
import * as api from "../../ipc/docker";
import type { SessionInfo } from "../../ipc/contracts";
import { safeError } from "../../ipc/client";
import { choose } from "../files/dialogs";
import { inputDialog, downOptions, inspection } from "./dialogs";
export function allowed(resource: api.Resource, action: api.Action) {
  if (resource.kind !== "container") return action === "remove";
  return (
    {
      start: ["created", "exited", "dead"],
      stop: ["running", "paused", "restarting"],
      restart: ["running", "exited"],
      pause: ["running"],
      unpause: ["paused"],
      remove: ["created", "exited", "dead"],
    } as Record<api.Action, string[]>
  )[action].includes(resource.state);
}
const labels: Record<api.Action | string, string> = {
  start: "Iniciar",
  stop: "Parar",
  restart: "Reiniciar",
  pause: "Pausar",
  unpause: "Retomar",
  remove: "Remover",
  up: "Up",
  down: "Down",
};
type Context = {
  kind: api.Kind | "compose";
  resources: api.Resource[];
  projects: api.Project[];
  loaded: Set<string>;
  query: string;
  message: string;
  busy: boolean;
};
type Job = {
  local: string;
  session: string;
  id?: string;
  name: string;
  sequence: number;
  state: string;
  stdout: string;
  stderr: string;
  out: TextDecoder;
  err: TextDecoder;
  truncated: boolean;
};
export class DockerPanel {
  private session?: SessionInfo;
  private alive = true;
  private visible = false;
  private contexts = new Map<string, Context>();
  private jobs = new Map<string, Job>();
  constructor(
    private generation: number,
    private host: HTMLElement,
    private report: (s: string) => void,
    private terminal: (session: string, resource: api.Resource) => void,
  ) {
    host.innerHTML =
      '<div class="docker-intro"><p class="eyebrow">DOCKER REMOTO</p><p class="muted docker-owner"></p></div><nav class="docker-nav" aria-label="Inventário Docker"><button data-kind="container">Containers</button><button data-kind="image">Imagens</button><button data-kind="volume">Volumes</button><button data-kind="compose">Compose</button></nav><div class="docker-toolbar"><input aria-label="Pesquisar recursos Docker" placeholder="Pesquisar por nome, ID ou projeto"><button class="refresh">Atualizar</button><button class="register" hidden>+ Registrar diretório</button></div><p class="docker-message" role="status"></p><div class="docker-resources"></div><div class="docker-jobs" aria-label="Operações Docker"></div>';
    host.querySelectorAll<HTMLButtonElement>("[data-kind]").forEach(
      (b) =>
        (b.onclick = () => {
          const ctx = this.current();
          if (!ctx) return;
          ctx.kind = b.dataset.kind as Context["kind"];
          ctx.query = "";
          host.querySelector("input")!.value = "";
          this.draw();
          if (!ctx.loaded.has(ctx.kind)) void this.refresh();
        }),
    );
    host.querySelector<HTMLInputElement>("input")!.oninput = (e) => {
      const ctx = this.current();
      if (ctx) {
        ctx.query = (e.target as HTMLInputElement).value;
        this.drawRows(ctx);
      }
    };
    host.querySelector<HTMLButtonElement>(".refresh")!.onclick = () =>
      void this.refresh();
    host.querySelector<HTMLButtonElement>(".register")!.onclick = () =>
      void this.register();
  }
  private current() {
    return this.session && this.contexts.get(this.session.id);
  }
  show(session?: SessionInfo, visible = this.visible) {
    this.session = session;
    this.visible = visible;
    if (session && !this.contexts.has(session.id))
      this.contexts.set(session.id, {
        kind: "container",
        resources: [],
        projects: [],
        loaded: new Set(),
        query: "",
        message: "",
        busy: false,
      });
    this.draw();
    const ctx = this.current();
    if (
      visible &&
      session?.state === "connected" &&
      ctx &&
      !ctx.loaded.has(ctx.kind) &&
      !ctx.busy
    )
      void this.refresh();
  }
  private async run(action: () => Promise<void>) {
    const ctx = this.current();
    if (!ctx || ctx.busy) return;
    ctx.busy = true;
    this.draw();
    try {
      await action();
    } catch (error) {
      ctx.message = safeError(error);
      this.report(ctx.message);
    } finally {
      ctx.busy = false;
      this.draw();
    }
  }
  async refresh() {
    const session = this.session;
    if (session?.state !== "connected") return;
    const ctx = this.current()!;
    await this.run(async () => {
      if (ctx.kind === "compose") {
        ctx.projects = await api.projects(this.generation, session.id);
        ctx.message = `${ctx.projects.length} projetos registrados · Compose v2 validado ao consultar ou executar`;
      } else {
        const result = await api.inventory(
          this.generation,
          session.id,
          ctx.kind,
        );
        ctx.resources = [
          ...ctx.resources.filter((r) => r.kind !== ctx.kind),
          ...result.resources,
        ];
        ctx.message = `Docker ${result.version} · ${result.resources.length} recursos · daemon local do servidor`;
      }
      ctx.loaded.add(ctx.kind);
    });
  }
  private draw() {
    if (!this.alive) return;
    const ctx = this.current();
    this.host.querySelector(".docker-owner")!.textContent = this.session
      ? `${this.session.profile.name} · ${this.session.profile.username}@${this.session.profile.host}:${this.session.profile.port}`
      : "Conecte uma sessão SSH para consultar Docker.";
    this.host.querySelector(".docker-message")!.textContent =
      this.session?.state !== "connected"
        ? "Sessão desconectada. Reconecte manualmente; comandos não serão repetidos."
        : ctx?.busy
          ? "Aguardando o servidor…"
          : (ctx?.message ?? "");
    this.host
      .querySelectorAll<HTMLButtonElement>("button")
      .forEach(
        (b) =>
          (b.disabled = this.session?.state !== "connected" || !!ctx?.busy),
      );
    this.host
      .querySelectorAll<HTMLButtonElement>("[data-kind]")
      .forEach((b) =>
        b.setAttribute("aria-pressed", String(b.dataset.kind === ctx?.kind)),
      );
    this.host.querySelector<HTMLElement>(".register")!.hidden =
      ctx?.kind !== "compose";
    if (ctx) this.drawRows(ctx);
    else this.host.querySelector(".docker-resources")!.replaceChildren();
    this.drawJobs();
  }
  private button(text: string, action: () => void, disabled = false) {
    const b = document.createElement("button");
    b.textContent = text;
    b.disabled = disabled;
    b.onclick = action;
    return b;
  }
  private drawRows(ctx: Context) {
    const list = this.host.querySelector(".docker-resources")!;
    list.replaceChildren();
    const session = this.session;
    if (!session) return;
    const disabled = ctx.busy || session.state !== "connected";
    const query = ctx.query.toLocaleLowerCase();
    let count = 0;
    if (ctx.kind === "compose") {
      for (const project of ctx.projects) {
        if (
          !`${project.name} ${project.path}`.toLocaleLowerCase().includes(query)
        )
          continue;
        count++;
        const row = document.createElement("article");
        row.className = "docker-resource";
        const title = document.createElement("h2"),
          path = document.createElement("p"),
          actions = document.createElement("div");
        title.textContent = project.name;
        path.textContent = project.path;
        actions.className = "docker-actions";
        for (const action of [
          "up",
          "start",
          "stop",
          "restart",
          "down",
        ] as api.ComposeAction[])
          actions.append(
            this.button(
              labels[action],
              () => void this.compose(session, project, action),
              disabled,
            ),
          );
        actions.append(
          this.button(
            "Consultar serviços",
            () =>
              void this.run(async () =>
                inspection(
                  `${project.name} · serviços`,
                  await api.projectStatus(
                    this.generation,
                    session.id,
                    project.id,
                  ),
                ),
              ),
            disabled,
          ),
          this.button(
            "Esquecer registro",
            () => void this.forget(session, project),
            disabled,
          ),
        );
        row.append(title, path, actions);
        list.append(row);
      }
    } else {
      for (const resource of ctx.resources) {
        if (resource.kind !== ctx.kind) continue;
        if (
          !`${resource.name} ${resource.id} ${resource.project} ${resource.image}`
            .toLocaleLowerCase()
            .includes(query)
        )
          continue;
        count++;
        const row = document.createElement("article");
        row.className = "docker-resource";
        const heading = document.createElement("div"),
          title = document.createElement("h2"),
          state = document.createElement("span"),
          id = document.createElement("code"),
          details = document.createElement("p"),
          actions = document.createElement("div");
        heading.className = "docker-resource-heading";
        title.textContent = resource.name;
        state.className = "resource-state";
        state.textContent = resource.state || resource.kind;
        id.textContent = resource.id;
        details.textContent = [
          resource.details,
          resource.image,
          resource.ports,
          resource.tags,
          resource.project ? `Projeto: ${resource.project}` : "",
        ]
          .filter(Boolean)
          .join(" · ");
        heading.append(title, state);
        actions.className = "docker-actions";
        actions.append(
          this.button(
            "Inspecionar",
            () =>
              void this.run(async () =>
                inspection(
                  `${resource.name} · ${session.profile.name}`,
                  await api.inspect(
                    this.generation,
                    session.id,
                    resource.handle,
                  ),
                ),
              ),
            disabled,
          ),
        );
        if (resource.kind === "container") {
          for (const action of [
            "start",
            "stop",
            "restart",
            "pause",
            "unpause",
          ] as api.Action[])
            actions.append(
              this.button(
                labels[action],
                () => void this.act(session, resource, action),
                disabled || !allowed(resource, action),
              ),
            );
          actions.append(
            this.button(
              "Logs",
              () => void this.startLog(session, resource),
              disabled,
            ),
            this.button(
              "Terminal /bin/sh",
              () => this.terminal(session.id, resource),
              disabled || resource.state !== "running",
            ),
          );
        }
        actions.append(
          this.button(
            "Remover",
            () => void this.act(session, resource, "remove"),
            disabled || !allowed(resource, "remove"),
          ),
        );
        row.append(heading, id, details, actions);
        list.append(row);
      }
    }
    if (!count) {
      const empty = document.createElement("p");
      empty.className = "empty";
      empty.textContent = ctx.loaded.has(ctx.kind)
        ? "Nenhum recurso encontrado."
        : "Use Atualizar para consultar este servidor.";
      list.append(empty);
    }
  }
  private async act(
    session: SessionInfo,
    resource: api.Resource,
    action: api.Action,
  ) {
    await this.run(async () => {
      const confirmation = await api.prepare(
        this.generation,
        session.id,
        resource.handle,
        action,
      );
      let typed: string | undefined;
      const message = `Servidor: ${confirmation.server}\nRecurso: ${confirmation.name}\nID: ${resource.id}\n${resource.kind === "volume" ? "Remover volume apaga dados permanentemente. Volumes em uso serão recusados." : action === "remove" ? "Recursos em uso serão recusados. Volumes não são removidos implicitamente." : `Executar ${labels[action]}?`}`;
      if (confirmation.needsName) {
        const name = await inputDialog(
          "Remover volume",
          message,
          `Digite ${confirmation.name}`,
          confirmation.name,
        );
        if (name === null) return;
        typed = name;
      } else if (
        (await choose(labels[action], message, [
          { label: "Cancelar", value: "cancel" },
          { label: "Confirmar", value: "apply", danger: action === "remove" },
        ])) !== "apply"
      )
        return;
      const result = await api.apply(
        this.generation,
        session.id,
        confirmation.token,
        typed,
      );
      this.result(session.id, `${labels[action]} · ${resource.name}`, result);
      if (result.exitCode === 0) {
        const ctx = this.contexts.get(session.id)!;
        ctx.resources = [
          ...ctx.resources.filter((r) => r.kind !== resource.kind),
          ...(await api.inventory(this.generation, session.id, resource.kind))
            .resources,
        ];
        ctx.message = "Ação concluída no servidor.";
      } else
        this.report("A ação falhou. Confira a saída e as dependências em uso.");
    });
  }
  private async register() {
    const session = this.session;
    if (!session) return;
    await this.run(async () => {
      const path = await inputDialog(
        "Registrar projeto Compose",
        `Servidor: ${session.profile.host}:${session.profile.port}\nO diretório precisa conter uma configuração Compose válida.`,
        "Diretório remoto absoluto",
      );
      if (!path) return;
      await api.register(this.generation, session.id, path);
      const ctx = this.contexts.get(session.id)!;
      ctx.projects = await api.projects(this.generation, session.id);
      ctx.loaded.add("compose");
      ctx.message = "Projeto validado e registrado no cofre criptografado.";
    });
  }
  private async forget(session: SessionInfo, project: api.Project) {
    await this.run(async () => {
      if (
        (await choose(
          "Esquecer projeto",
          `${project.name} · ${project.path}\nSomente o registro local será removido.`,
          [
            { label: "Cancelar", value: "cancel" },
            { label: "Esquecer registro", value: "remove" },
          ],
        )) !== "remove"
      )
        return;
      await api.removeProject(this.generation, session.id, project.id);
      this.contexts.get(session.id)!.projects = await api.projects(
        this.generation,
        session.id,
      );
    });
  }
  private async compose(
    session: SessionInfo,
    project: api.Project,
    action: api.ComposeAction,
  ) {
    await this.run(async () => {
      let volumes = false;
      if (action === "down") {
        const options = await downOptions(
          `${session.profile.host}:${session.profile.port}`,
          project.name,
        );
        if (!options) return;
        volumes = options.volumes;
      }
      const confirmation = await api.prepareCompose(
        this.generation,
        session.id,
        project.id,
        action,
        volumes,
      );
      let typed: string | undefined, volumeConfirmation: string | undefined;
      const message = `Servidor: ${confirmation.server}\nProjeto: ${confirmation.project}\nDiretório: ${confirmation.path}`;
      if (action === "down") {
        const name = await inputDialog(
          "Confirmar down",
          `${message}\nContainers e redes serão removidos. ${volumes ? "Volumes também serão removidos após uma autorização adicional." : "Volumes serão preservados."}`,
          `Digite ${confirmation.project}`,
          confirmation.project,
        );
        if (name === null) return;
        typed = name;
      } else if (
        (await choose(
          `Compose ${action}`,
          `${message}\nExecutar esta ação no projeto?`,
          [
            { label: "Cancelar", value: "cancel" },
            { label: "Executar", value: "apply" },
          ],
        )) !== "apply"
      )
        return;
      if (volumes) {
        const exact = `REMOVER VOLUMES ${confirmation.project}`;
        const consent = await inputDialog(
          "Autorizar perda de dados dos volumes",
          `${message}\nEsta autorização adicional permite remover os volumes do projeto. Os dados podem ser perdidos permanentemente.`,
          `Digite ${exact}`,
          exact,
        );
        if (consent === null) return;
        volumeConfirmation = consent;
      }
      const job = this.job(session.id, `Compose ${action} · ${project.name}`);
      try {
        job.id = await api.applyCompose(
          this.generation,
          session.id,
          confirmation,
          typed,
          volumeConfirmation,
          this.channel(job),
        );
      } catch (e) {
        job.state = "failed";
        job.stderr = safeError(e);
        throw e;
      }
      this.drawJobs();
    });
  }
  private job(session: string, name: string): Job {
    for (const [key, value] of this.jobs) {
      if (this.jobs.size < 32) break;
      if (!["opening", "running"].includes(value.state)) this.jobs.delete(key);
    }
    if (this.jobs.size >= 32) throw new Error("Limite de operações");
    const job: Job = {
      local: crypto.randomUUID(),
      session,
      name,
      sequence: 0,
      state: "opening",
      stdout: "",
      stderr: "",
      out: new TextDecoder(),
      err: new TextDecoder(),
      truncated: false,
    };
    this.jobs.set(job.local, job);
    return job;
  }
  private result(session: string, name: string, result: api.Output) {
    const job = this.job(session, name);
    job.state = result.exitCode === 0 ? "completed" : "failed";
    job.stdout = result.stdout.slice(-262144);
    job.stderr = result.stderr.slice(-262144);
    job.truncated = result.stdout.length + result.stderr.length > 262144;
    this.drawJobs();
  }
  private channel(job: Job) {
    const channel = new Channel<api.DockerEvent>();
    channel.onmessage = (event) => {
      if (
        !this.alive ||
        event.generation !== this.generation ||
        event.sessionId !== job.session ||
        event.sequence !== job.sequence + 1 ||
        (job.id && job.id !== event.operationId)
      )
        return;
      job.id = event.operationId;
      job.sequence = event.sequence;
      if (event.kind === "stdout" || event.kind === "stderr") {
        job.state = "running";
        const text = (event.kind === "stderr" ? job.err : job.out).decode(
          Uint8Array.from(event.bytes),
          { stream: true },
        );
        if (event.kind === "stderr") job.stderr += text;
        else job.stdout += text;
        if (job.stdout.length + job.stderr.length > 262144) {
          job.truncated = true;
          job.stdout = job.stdout.slice(-131072);
          job.stderr = job.stderr.slice(-131072);
        }
        void api
          .streamAction(
            this.generation,
            job.session,
            event.operationId,
            event.sequence,
          )
          .catch(() => {});
      } else {
        job.state = event.kind;
        if (event.message) job.stderr += `\n${event.message}`;
        if (event.kind === "completed") {
          const ctx = this.contexts.get(job.session);
          ctx?.loaded.delete("container");
          ctx?.loaded.delete("image");
          ctx?.loaded.delete("volume");
          if (this.session?.id === job.session && !ctx?.busy)
            void this.refresh();
        }
        if (event.kind === "completed")
          this.report(
            "Operação Docker concluída. Use Atualizar para consultar o estado.",
          );
      }
      this.drawJobs();
    };
    return channel;
  }
  private async startLog(session: SessionInfo, resource: api.Resource) {
    try {
      const job = this.job(session.id, `Logs · ${resource.name}`);
      job.id = await api.logs(
        this.generation,
        session.id,
        resource.handle,
        this.channel(job),
      );
      this.drawJobs();
    } catch (error) {
      this.report(safeError(error));
    }
  }
  private drawJobs() {
    if (!this.alive) return;
    const list = this.host.querySelector(".docker-jobs")!;
    list.replaceChildren();
    for (const job of [...this.jobs.values()]
      .filter((j) => j.session === this.session?.id)
      .slice(-8)
      .reverse()) {
      const details = document.createElement("details");
      details.className = "docker-job";
      details.open = true;
      const summary = document.createElement("summary");
      summary.textContent = `${job.name} · ${{ opening: "Abrindo", running: "Em andamento", completed: "Concluído", failed: "Falhou", cancelled: "Cancelado" }[job.state] ?? job.state}`;
      const text = document.createElement("pre");
      text.textContent = `${job.truncated ? "[Saída anterior descartada pelo limite de 256 KiB]\n" : ""}${job.stdout}${job.stderr ? `\n[stderr]\n${job.stderr}` : ""}`;
      details.append(summary, text);
      if (["opening", "running"].includes(job.state))
        details.append(
          this.button("Cancelar acompanhamento", () => {
            if (job.id)
              void api
                .streamAction(this.generation, job.session, job.id)
                .catch(() => {});
            this.report(
              "Cancelamento solicitado. Efeitos já executados podem permanecer no servidor.",
            );
          }),
        );
      list.append(details);
    }
  }
  closeSession(session: string) {
    this.contexts.delete(session);
    for (const [id, job] of this.jobs)
      if (job.session === session) {
        if (job.id && ["running", "opening"].includes(job.state))
          void api
            .streamAction(this.generation, session, job.id)
            .catch(() => {});
        this.jobs.delete(id);
      }
  }
  dispose() {
    this.alive = false;
    for (const session of this.contexts.keys()) this.closeSession(session);
  }
}
