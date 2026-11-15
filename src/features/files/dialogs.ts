import { dialog } from "../connections/dialogs";
export function choose(
  title: string,
  message: string,
  actions: { label: string; value: string; danger?: boolean }[],
): Promise<string | null> {
  return new Promise((resolve) => {
    const d = dialog(title);
    const text = document.createElement("p");
    text.className = "muted dialog-message";
    text.textContent = message;
    const buttons = document.createElement("div");
    buttons.className = "dialog-actions";
    for (const action of actions) {
      const button = document.createElement("button");
      button.textContent = action.label;
      button.className = action.danger ? "danger" : "";
      button.onclick = () => {
        resolve(action.value);
        d.element.close();
      };
      buttons.append(button);
    }
    d.body.append(text, buttons);
    const previous = d.element.onclose;
    d.element.onclose = (event) => {
      previous?.call(d.element, event);
      resolve(null);
    };
    d.show();
    buttons.querySelector("button")?.focus();
  });
}
export function askName(
  title: string,
  message: string,
  initial = "",
): Promise<string | null> {
  return new Promise((resolve) => {
    const d = dialog(title);
    d.body.innerHTML =
      '<p class="muted dialog-message"></p><form class="vault-form"><label>Nome<input name="name" required maxlength="255" autocomplete="off"></label><div class="dialog-actions"><button type="button">Cancelar</button><button type="submit" class="primary">Continuar</button></div></form>';
    d.body.querySelector("p")!.textContent = message;
    const input = d.body.querySelector("input")!;
    input.value = initial;
    d.body.querySelector<HTMLButtonElement>('[type="button"]')!.onclick = () =>
      d.element.close();
    d.body.querySelector("form")!.onsubmit = (event) => {
      event.preventDefault();
      if (
        !input.value ||
        [".", ".."].includes(input.value) ||
        input.value.includes("/")
      ) {
        input.setCustomValidity("Use um nome simples, sem barras.");
        input.reportValidity();
        return;
      }
      resolve(input.value);
      d.element.close();
    };
    input.oninput = () => input.setCustomValidity("");
    const previous = d.element.onclose;
    d.element.onclose = (e) => {
      previous?.call(d.element, e);
      resolve(null);
    };
    d.show();
    input.focus();
    input.select();
  });
}
