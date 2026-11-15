import { dialog } from "../connections/dialogs";
export function inputDialog(
  title: string,
  message: string,
  label: string,
  expected?: string,
): Promise<string | null> {
  return new Promise((resolve) => {
    const d = dialog(title);
    const description = document.createElement("p");
    description.className = "muted dialog-message";
    description.textContent = message;
    const form = document.createElement("form");
    form.className = "vault-form";
    const l = document.createElement("label"),
      input = document.createElement("input");
    l.textContent = label;
    input.required = true;
    input.maxLength = 4096;
    input.autocomplete = "off";
    l.append(input);
    const cancel = document.createElement("button"),
      submit = document.createElement("button");
    cancel.type = "button";
    cancel.textContent = "Cancelar";
    submit.type = "submit";
    submit.textContent = "Confirmar";
    submit.className = expected ? "danger" : "primary";
    const actions = document.createElement("div");
    actions.className = "dialog-actions";
    actions.append(cancel, submit);
    form.append(l, actions);
    form.onsubmit = (e) => {
      e.preventDefault();
      if (expected !== undefined && input.value !== expected) {
        input.setCustomValidity("Digite exatamente o texto solicitado.");
        input.reportValidity();
        return;
      }
      resolve(input.value);
      d.element.close();
    };
    input.oninput = () => input.setCustomValidity("");
    cancel.onclick = () => d.element.close();
    const close = d.element.onclose;
    d.element.onclose = (e) => {
      close?.call(d.element, e);
      resolve(null);
    };
    d.body.append(description, form);
    d.show();
    input.focus();
  });
}
export function downOptions(
  server: string,
  project: string,
): Promise<{ volumes: boolean } | null> {
  return new Promise((resolve) => {
    const d = dialog("Encerrar projeto Compose");
    d.body.innerHTML =
      '<p class="muted dialog-message"></p><label class="volume-option"><input type="checkbox"> Remover também os volumes do projeto (perda de dados)</label><div class="dialog-actions"><button class="cancel">Cancelar</button><button class="danger">Revisar down</button></div>';
    d.body.querySelector("p")!.textContent =
      `Servidor: ${server}\nProjeto: ${project}\nDown remove containers e redes. Por padrão, os volumes são preservados.`;
    d.body.querySelector<HTMLButtonElement>(".cancel")!.onclick = () =>
      d.element.close();
    d.body.querySelector<HTMLButtonElement>(".danger")!.onclick = () => {
      resolve({ volumes: d.body.querySelector("input")!.checked });
      d.element.close();
    };
    const close = d.element.onclose;
    d.element.onclose = (e) => {
      close?.call(d.element, e);
      resolve(null);
    };
    d.show();
  });
}
export function inspection(title: string, value: unknown) {
  const d = dialog(title),
    text = document.createElement("pre");
  text.className = "docker-inspection";
  text.textContent = JSON.stringify(value, null, 2);
  d.body.append(text);
  d.show();
}
