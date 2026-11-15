import { brand } from "../../ui/brand";
import { vaultAction, safeError, resetChallenge } from "../../ipc/client";
import type { VaultStatus, VaultRequest } from "../../ipc/contracts";
export type VaultMode = "create" | "unlock" | "change_password" | "reset";
const titles = {
  create: "Um lugar seguro para suas conexões.",
  unlock: "Seu espaço, protegido.",
  change_password: "Trocar senha mestra",
  reset: "Redefinir o cofre",
};
export function vaultForm(
  container: HTMLElement,
  mode: VaultMode,
  done: (status: VaultStatus) => void,
) {
  container.innerHTML = `<div class="vault-mark">${brand("full")}</div><p class="eyebrow">UNNY SSH / COFRE LOCAL</p><h1>${titles[mode]}</h1>
    <p class="muted">${mode === "reset" ? "Todos os perfis, credenciais e rascunhos serão perdidos. Essa ação não pode ser desfeita." : mode === "unlock" ? "Digite a senha mestra para abrir seu ambiente de trabalho." : "Sua senha protege os dados neste computador. Não há recuperação de senha mestra."}</p>
    <form class="vault-form">
    ${mode === "change_password" ? '<label>Senha atual<input name="old" type="password" autocomplete="current-password" required maxlength="1024"></label>' : ""}
    <label>${mode === "unlock" ? "Senha mestra" : "Nova senha mestra"}<input name="password" type="password" autocomplete="${mode === "unlock" ? "current-password" : "new-password"}" required ${mode === "unlock" ? "" : 'minlength="12"'} maxlength="1024"></label>
    ${mode !== "unlock" ? '<label>Confirme a senha<input name="repeat" type="password" autocomplete="new-password" required maxlength="1024"></label><small>Use pelo menos 12 caracteres.</small>' : ""}
    ${mode === "reset" ? '<label>Digite APAGAR MEU COFRE<input name="confirmation" autocomplete="off" required pattern="APAGAR MEU COFRE"></label>' : ""}
    <p class="form-error" role="alert"></p><button class="primary" type="submit">${mode === "unlock" ? "Desbloquear cofre" : mode === "create" ? "Criar cofre" : mode === "reset" ? "Apagar dados e criar cofre" : "Salvar nova senha"}<span aria-hidden="true"> →</span></button>
    </form><p class="privacy-note">Argon2id · Criptografia autenticada · Somente neste dispositivo</p>`;
  const form = container.querySelector<HTMLFormElement>("form")!;
  const error = container.querySelector<HTMLElement>(".form-error")!;
  form.onsubmit = async (event) => {
    event.preventDefault();
    const field = (name: string) =>
      form.elements.namedItem(name) as HTMLInputElement;
    const password = field("password").value;
    if (mode !== "unlock" && password !== field("repeat").value) {
      error.textContent = "As senhas não coincidem.";
      field("repeat").focus();
      return;
    }
    const request: VaultRequest =
      mode === "change_password"
        ? {
            action: mode,
            old_password: field("old").value,
            new_password: password,
          }
        : mode === "reset"
          ? {
              action: mode,
              password,
              confirmation: field("confirmation").value,
              confirmation_token: "",
            }
          : { action: mode, password };
    form.reset();
    const button = form.querySelector("button")!;
    button.disabled = true;
    form.setAttribute("aria-busy", "true");
    error.textContent = "Protegendo seus dados…";
    try {
      if (request.action === "reset")
        request.confirmation_token = await resetChallenge();
      done(await vaultAction(request));
    } catch (e) {
      error.textContent = safeError(e);
      field(mode === "change_password" ? "old" : "password").focus();
    } finally {
      if ("password" in request) request.password = "";
      if ("old_password" in request) {
        request.old_password = "";
        request.new_password = "";
      }
      button.disabled = false;
      form.removeAttribute("aria-busy");
    }
  };
}
