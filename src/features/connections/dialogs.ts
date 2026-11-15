import * as api from "../../ipc/connections";
import { safeError } from "../../ipc/client";
import type {
  Profile,
  ProfileInput,
  CredentialInput,
  AuthMode,
  SessionInfo,
} from "../../ipc/contracts";
export function dialog(title: string) {
  const previous = document.activeElement as HTMLElement | null;
  const element = document.createElement("dialog");
  element.className = "connection-dialog";
  element.setAttribute("aria-label", title);
  element.innerHTML =
    '<button class="dialog-close" aria-label="Fechar diálogo">×</button><h1></h1><div class="dialog-body"></div>';
  element.querySelector("h1")!.textContent = title;
  element.querySelector<HTMLButtonElement>("button")!.onclick = () =>
    element.close();
  element.onclose = () => {
    for (const input of element.querySelectorAll<HTMLInputElement>(
      'input[type="password"]',
    ))
      input.value = "";
    element.remove();
    if (previous?.isConnected) previous.focus();
  };
  document.querySelector("#app")!.append(element);
  return {
    element,
    body: element.querySelector<HTMLDivElement>(".dialog-body")!,
    show: () => {
      for (const input of element.querySelectorAll("input")) {
        input.setAttribute("autocorrect", "off");
        input.setAttribute("autocapitalize", "off");
        input.spellcheck = false;
      }
      element.showModal();
    },
  };
}
function credentialFields(
  container: HTMLElement,
  mode: AuthMode,
  generation: number,
) {
  container.innerHTML =
    mode === "password"
      ? '<label>Senha SSH<input name="secret" type="password" autocomplete="off" maxlength="4096" required></label>'
      : mode === "key"
        ? '<button type="button" class="import-key">Selecionar chave privada…</button><p class="import-name muted" role="status">Nenhuma chave selecionada</p><label>Passphrase (se houver)<input name="passphrase" type="password" autocomplete="off" maxlength="4096"></label>'
        : '<p class="muted">O SSH Agent nativo fornece a assinatura. Nenhuma chave será copiada para o aplicativo.</p>';
  let keyImport: string | undefined;
  const button = container.querySelector<HTMLButtonElement>(".import-key");
  if (button)
    button.onclick = async () => {
      button.disabled = true;
      try {
        const key = await api.importKey(generation);
        if (key && container.isConnected) {
          keyImport = key.id;
          container.querySelector(".import-name")!.textContent = key.name;
        }
      } catch (error) {
        if (container.isConnected)
          container.querySelector(".import-name")!.textContent =
            safeError(error);
      } finally {
        button.disabled = false;
      }
    };
  return (): CredentialInput | undefined => {
    if (mode === "agent") return undefined;
    if (mode === "password") {
      const input = container.querySelector<HTMLInputElement>("input")!;
      const password = input.value;
      input.value = "";
      return { password };
    }
    if (!keyImport)
      throw new Error("Selecione uma chave privada antes de continuar.");
    const input = container.querySelector<HTMLInputElement>("input")!;
    const credential = { keyImport, passphrase: input.value };
    input.value = "";
    keyImport = undefined;
    return credential;
  };
}
function errorText(error: unknown) {
  return error instanceof Error ? error.message : safeError(error);
}
function wipe(credential: CredentialInput | undefined) {
  if (credential) {
    credential.password = "";
    credential.passphrase = "";
    credential.keyImport = undefined;
  }
}
export function editProfile(
  generation: number,
  profile: Profile | undefined,
  done: () => void,
) {
  const d = dialog(profile ? "Editar conexão" : "Nova conexão");
  d.body.innerHTML =
    '<p class="muted">Os dados da conexão ficam no cofre local. Alterações valem para novas sessões.</p><form class="vault-form"><label>Nome<input name="name" required maxlength="120" autocomplete="off"></label><div class="form-row"><label>Host / IP<input name="host" required maxlength="253" autocomplete="off"></label><label>Porta<input name="port" type="number" min="1" max="65535" required value="22"></label></div><label>Usuário<input name="username" required maxlength="128" autocomplete="off"></label><label>Diretório inicial (opcional)<input name="directory" placeholder="/home/usuario" maxlength="4096" autocomplete="off"></label><label>Autenticação<select name="auth"><option value="password">Senha</option><option value="key">Chave privada</option><option value="agent">SSH Agent</option></select></label><label>Credencial<select name="policy"><option value="forget">Informar ao conectar — não salvar</option><option value="keep">Manter credencial salva</option><option value="save">Salvar nova credencial no cofre</option></select></label><div class="credential-fields"></div><p class="form-error" role="alert"></p><button class="primary" type="submit">Salvar conexão →</button></form>';
  const form = d.body.querySelector<HTMLFormElement>("form")!;
  const field = (name: string) =>
    form.elements.namedItem(name) as HTMLInputElement;
  if (profile) {
    for (const [name, value] of Object.entries({
      name: profile.name,
      host: profile.host,
      port: String(profile.port),
      username: profile.username,
      directory: profile.initialDirectory ?? "",
      auth: profile.authMode,
    }))
      field(name).value = value;
    field("policy").value = profile.hasSavedSecret ? "keep" : "forget";
  }
  const keep = form.querySelector<HTMLOptionElement>('option[value="keep"]')!;
  keep.disabled = !profile?.hasSavedSecret;
  let readCredential: () => CredentialInput | undefined = () => undefined;
  const update = () => {
    const agent = field("auth").value === "agent";
    field("policy").disabled = agent;
    const container = form.querySelector<HTMLDivElement>(".credential-fields")!;
    if (!agent && field("policy").value === "save")
      readCredential = credentialFields(
        container,
        field("auth").value as AuthMode,
        generation,
      );
    else {
      container.textContent = agent
        ? "O Agent será consultado ao conectar."
        : "Senhas, chaves e passphrases não aparecem na listagem.";
      readCredential = () => undefined;
    }
  };
  field("auth").onchange = () => {
    if (profile && field("auth").value !== profile.authMode) {
      field("policy").value = "forget";
      keep.disabled = true;
    } else keep.disabled = !profile?.hasSavedSecret;
    update();
  };
  field("policy").onchange = update;
  update();
  form.onsubmit = async (event) => {
    event.preventDefault();
    const error = form.querySelector<HTMLElement>(".form-error")!;
    let credential: CredentialInput | undefined;
    const value: ProfileInput = {
      id: profile?.id ?? null,
      name: field("name").value,
      host: field("host").value,
      port: Number(field("port").value),
      username: field("username").value,
      authMode: field("auth").value as AuthMode,
      initialDirectory: field("directory").value || null,
    };
    const button = form.querySelector<HTMLButtonElement>('[type="submit"]')!;
    try {
      credential = readCredential();
      button.disabled = true;
      error.textContent = "Salvando no cofre…";
      await api.profilePut(
        generation,
        value,
        value.authMode === "agent"
          ? "forget"
          : (field("policy").value as "keep" | "save" | "forget"),
        credential,
      );
      d.element.close();
      done();
    } catch (e) {
      error.textContent = errorText(e);
    } finally {
      wipe(credential);
      button.disabled = false;
    }
  };
  d.show();
  field("name").focus();
}
export function transientCredential(
  generation: number,
  profile: Profile,
  connect: (credential?: CredentialInput) => Promise<void>,
) {
  const d = dialog("Autenticar conexão");
  d.body.innerHTML =
    '<p class="connection-target"></p><p class="muted">Esta credencial será usada apenas nesta conexão, sem gravar no disco.</p><form class="vault-form"><div class="credential-fields"></div><p class="form-error" role="alert"></p><button class="primary" type="submit">Conectar →</button></form>';
  d.body.querySelector(".connection-target")!.textContent =
    `${profile.username}@${profile.host}:${profile.port}`;
  const read = credentialFields(
    d.body.querySelector(".credential-fields")!,
    profile.authMode,
    generation,
  );
  const form = d.body.querySelector<HTMLFormElement>("form")!;
  form.onsubmit = async (event) => {
    event.preventDefault();
    let credential: CredentialInput | undefined;
    const button = form.querySelector(
      "button[type=submit]",
    ) as HTMLButtonElement;
    try {
      credential = read();
      button.disabled = true;
      await connect(credential);
      d.element.close();
    } catch (e) {
      form.querySelector(".form-error")!.textContent = errorText(e);
    } finally {
      wipe(credential);
      button.disabled = false;
    }
  };
  d.show();
  d.body.querySelector<HTMLInputElement>("input")?.focus();
}
export async function removeProfile(
  generation: number,
  profile: Profile,
  done: () => void,
) {
  const token = await api.profileDeletePrepare(generation, profile.id!);
  const d = dialog("Excluir conexão?");
  d.body.innerHTML =
    '<p class="connection-target"></p><p class="muted">A conexão e a credencial salva serão removidas do cofre. Sessões já abertas mantêm seu contexto.</p><p class="form-error" role="alert"></p><div class="dialog-actions"><button class="cancel">Cancelar</button><button class="danger">Excluir conexão</button></div>';
  d.body.querySelector(".connection-target")!.textContent =
    `${profile.name} · ${profile.username}@${profile.host}:${profile.port}`;
  d.body.querySelector<HTMLButtonElement>(".cancel")!.onclick = () =>
    d.element.close();
  d.body.querySelector<HTMLButtonElement>(".danger")!.onclick = async (
    event,
  ) => {
    const button = event.currentTarget as HTMLButtonElement;
    button.disabled = true;
    try {
      await api.profileDelete(generation, profile.id!, token);
      d.element.close();
      done();
    } catch (e) {
      d.body.querySelector(".form-error")!.textContent = safeError(e);
      button.disabled = false;
    }
  };
  d.show();
  d.body.querySelector<HTMLButtonElement>(".cancel")!.focus();
}
export function hostConfirmation(
  generation: number,
  session: SessionInfo,
  done: () => void,
) {
  const challenge = session.challenge!;
  const d = dialog(
    challenge.changed
      ? "A identidade do servidor mudou"
      : "Confirmar identidade do servidor",
  );
  d.body.innerHTML =
    '<p class="connection-target"></p><p class="host-warning"></p><dl class="fingerprints"><dt>Algoritmo</dt><dd class="algorithm"></dd><dt>Impressão digital recebida</dt><dd class="fingerprint"></dd></dl><p class="muted">Compare esta impressão digital com uma fonte confiável do administrador. Expira em até cinco minutos. Nenhuma credencial foi enviada.</p><form class="vault-form"><div class="revision"></div><p class="form-error" role="alert"></p><div class="dialog-actions"><button type="button" class="reject">Rejeitar</button><button type="submit" class="primary">Confiar e conectar</button></div></form>';
  const address = `${challenge.host}:${challenge.port}`;
  d.body.querySelector(".connection-target")!.textContent = address;
  d.body.querySelector(".algorithm")!.textContent = challenge.algorithm;
  d.body.querySelector(".fingerprint")!.textContent = challenge.fingerprint;
  d.body.querySelector(".host-warning")!.textContent = challenge.changed
    ? "A chave recebida difere da identidade salva. Isso pode indicar troca de servidor ou interceptação. Só substitua após verificar a causa."
    : "Este servidor ainda não tem uma identidade conhecida neste cofre.";
  if (challenge.changed) {
    const dt = document.createElement("dt"),
      dd = document.createElement("dd");
    dt.textContent = "Impressão digital anterior";
    dd.textContent = challenge.previousFingerprint;
    d.body.querySelector("dl")!.append(dt, dd);
    d.body.querySelector(".revision")!.innerHTML =
      '<label>Para substituir, digite o endereço acima<input name="revision" required autocomplete="off"></label>';
  }
  let submitted = false;
  const decide = async (accept: boolean) => {
    if (submitted) return;
    submitted = true;
    const revision =
      d.body.querySelector<HTMLInputElement>('[name="revision"]')?.value ?? "";
    try {
      await api.confirmHost(
        generation,
        session.id,
        challenge.id,
        accept,
        revision,
      );
      d.element.close();
      done();
    } catch (e) {
      submitted = false;
      d.body.querySelector(".form-error")!.textContent = safeError(e);
    }
  };
  d.body.querySelector<HTMLFormElement>("form")!.onsubmit = (e) => {
    e.preventDefault();
    const revision =
      d.body.querySelector<HTMLInputElement>('[name="revision"]');
    if (challenge.changed && revision?.value !== address) {
      d.body.querySelector(".form-error")!.textContent =
        "Digite exatamente o endereço do servidor.";
      return;
    }
    void decide(true);
  };
  d.body.querySelector<HTMLButtonElement>(".reject")!.onclick = () =>
    void decide(false);
  d.element.querySelector<HTMLButtonElement>(".dialog-close")!.onclick = () =>
    void decide(false);
  d.element.oncancel = (e) => {
    e.preventDefault();
    void decide(false);
  };
  d.show();
  d.body.querySelector<HTMLButtonElement>(".reject")!.focus();
}
