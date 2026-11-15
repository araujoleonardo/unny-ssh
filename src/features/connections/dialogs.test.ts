// @vitest-environment jsdom
import { beforeEach, expect, test, vi } from "vitest";
vi.mock("../../ipc/connections", () => ({
  profilePut: vi.fn().mockResolvedValue({}),
  importKey: vi.fn().mockResolvedValue({ id: "import", name: "fixture" }),
  profileDeletePrepare: vi.fn().mockResolvedValue("token"),
  profileDelete: vi.fn().mockResolvedValue(undefined),
}));
import * as api from "../../ipc/connections";
import { editProfile, transientCredential, removeProfile } from "./dialogs";
import type { Profile } from "../../ipc/contracts";
beforeEach(() => {
  document.body.innerHTML = '<div id="app"></div>';
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
  HTMLDialogElement.prototype.close = function () {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  };
  vi.clearAllMocks();
});
const profile: Profile = {
  id: "profile",
  name: "Fixture",
  host: "example.invalid",
  port: 22,
  username: "fixture",
  authMode: "password",
  initialDirectory: null,
  hasSavedSecret: true,
};
test("saving credentials requires an explicit choice and supports all three modes", () => {
  editProfile(1, undefined, vi.fn());
  const form = document.querySelector("form")!;
  const auth = form.elements.namedItem("auth") as HTMLSelectElement;
  const policy = form.elements.namedItem("policy") as HTMLSelectElement;
  expect(policy.value).toBe("forget");
  expect(document.querySelector('[name="secret"]')).toBeNull();
  policy.value = "save";
  policy.dispatchEvent(new Event("change"));
  expect(document.querySelector('[name="secret"]')).not.toBeNull();
  auth.value = "key";
  auth.dispatchEvent(new Event("change"));
  expect(document.querySelector(".import-key")).not.toBeNull();
  auth.value = "agent";
  auth.dispatchEvent(new Event("change"));
  expect(policy.disabled).toBe(true);
  expect(document.querySelector('input[type="password"]')).toBeNull();
});
test("transient credential clears its field before connecting", async () => {
  let fieldWasCleared = false;
  const connect = vi.fn(async () => {
    fieldWasCleared =
      (document.querySelector('[name="secret"]') as HTMLInputElement).value ===
      "";
  });
  transientCredential(1, profile, connect);
  const input = document.querySelector<HTMLInputElement>('[name="secret"]')!;
  input.value = "synthetic-only";
  document
    .querySelector("form")!
    .dispatchEvent(new Event("submit", { cancelable: true }));
  expect(fieldWasCleared).toBe(true);
  await vi.waitFor(() => expect(connect).toHaveBeenCalled());
});
test("cancelled deletion does not mutate the vault", async () => {
  await removeProfile(1, profile, vi.fn());
  expect(document.querySelector(".connection-target")!.textContent).toContain(
    "example.invalid",
  );
  document.querySelector<HTMLButtonElement>(".cancel")!.click();
  expect(api.profileDelete).not.toHaveBeenCalled();
});

test.each(["password", "key", "agent"] as const)(
  "profile form submits %s with an explicit credential policy",
  async (mode) => {
    const done = vi.fn();
    let snapshot: unknown[] = [];
    vi.mocked(api.profilePut).mockImplementationOnce(async (...args) => {
      snapshot = structuredClone(args);
      const sensitive = document.querySelector<HTMLInputElement>(
        'input[type="password"]',
      );
      if (sensitive) expect(sensitive.value).toBe("");
      return { ...profile, authMode: mode };
    });
    editProfile(1, undefined, done);
    const form = document.querySelector<HTMLFormElement>("form")!;
    const field = (name: string) =>
      form.elements.namedItem(name) as HTMLInputElement;
    field("name").value = "Fixture";
    field("host").value = "example.invalid";
    field("username").value = "fixture";
    field("auth").value = mode;
    field("auth").dispatchEvent(new Event("change"));
    if (mode !== "agent") {
      field("policy").value = "save";
      field("policy").dispatchEvent(new Event("change"));
      if (mode === "password") field("secret").value = "synthetic-only";
      else {
        document.querySelector<HTMLButtonElement>(".import-key")!.click();
        await vi.waitFor(() =>
          expect(document.querySelector(".import-name")!.textContent).toBe(
            "fixture",
          ),
        );
        field("passphrase").value = "synthetic passphrase";
      }
    }
    form.dispatchEvent(new Event("submit", { cancelable: true }));
    await vi.waitFor(() => expect(done).toHaveBeenCalled());
    expect(snapshot[2]).toBe(mode === "agent" ? "forget" : "save");
    expect((snapshot[1] as { authMode: string }).authMode).toBe(mode);
    if (mode === "password")
      expect(snapshot[3]).toEqual({ password: "synthetic-only" });
    if (mode === "key")
      expect(snapshot[3]).toEqual({
        keyImport: "import",
        passphrase: "synthetic passphrase",
      });
    if (mode === "agent") expect(snapshot[3]).toBeUndefined();
  },
);
