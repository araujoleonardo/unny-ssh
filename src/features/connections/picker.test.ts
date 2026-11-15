// @vitest-environment jsdom
import { expect, test, vi } from "vitest";
import { ConnectionPicker } from "./picker";
import type { Profile, SessionInfo } from "../../ipc/contracts";
test("picker filters saved connections, switches sessions and requests a separate session explicitly", () => {
  document.body.innerHTML =
    '<button id="trigger"></button><div id="menu"></div>';
  const trigger = document.querySelector<HTMLButtonElement>("#trigger")!,
    menu = document.querySelector<HTMLElement>("#menu")!;
  const actions = {
    create: vi.fn(),
    manage: vi.fn(),
    select: vi.fn(),
    close: vi.fn(),
    connect: vi.fn(),
  };
  const profile = {
    id: "p",
    name: "Sandbox",
    host: "localhost",
    port: 22,
    username: "tester",
  } as Profile;
  const session: SessionInfo = {
    id: "s",
    profile,
    state: "connected",
    generation: 1,
    errorCode: null,
    errorMessage: null,
    challenge: null,
    sftp: true,
  };
  const picker = new ConnectionPicker(trigger, menu, actions);
  picker.update([profile], [session], "s");
  trigger.click();
  expect(menu.hidden).toBe(false);
  expect(trigger.textContent).toContain("Sandbox");
  const input = menu.querySelector("input")!;
  input.value = "missing";
  input.dispatchEvent(new Event("input"));
  expect(menu.textContent).toContain("Nenhuma conexão encontrada");
  input.value = "";
  input.dispatchEvent(new Event("input"));
  menu.querySelector<HTMLButtonElement>(".picker-row button")!.click();
  expect(actions.select).toHaveBeenCalledWith("s");
  expect(menu.hidden).toBe(true);
  trigger.click();
  menu
    .querySelector<HTMLButtonElement>('[aria-label="Nova sessão de Sandbox"]')!
    .click();
  expect(actions.connect).toHaveBeenCalledWith(profile, true);
  trigger.click();
  menu.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
  );
  expect(menu.hidden).toBe(true);
  expect(document.activeElement).toBe(trigger);
  picker.dispose();
});
