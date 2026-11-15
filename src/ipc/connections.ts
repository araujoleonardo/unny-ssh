import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  CredentialInput,
  Profile,
  ProfileInput,
  SessionInfo,
  TerminalEvent,
  TerminalCommand,
} from "./contracts";
export function profilesList(generation: number) {
  return invoke<Profile[]>("profiles_list", { request: { generation } });
}
export function profilePut(
  generation: number,
  profile: ProfileInput,
  secretPolicy: "keep" | "save" | "forget",
  credential?: CredentialInput,
) {
  return invoke<Profile>("profile_put", {
    request: { generation, profile, secretPolicy, credential },
  });
}
export function profileDuplicate(generation: number, profileId: string) {
  return invoke<Profile>("profile_duplicate", {
    request: { generation, profileId },
  });
}
export function profileDeletePrepare(generation: number, profileId: string) {
  return invoke<string>("profile_delete_prepare", {
    request: { generation, profileId },
  });
}
export function profileDelete(
  generation: number,
  profileId: string,
  token: string,
) {
  return invoke<void>("profile_delete", {
    request: { generation, profileId, token },
  });
}
export function importKey(generation: number) {
  return invoke<{ id: string; name: string } | null>("key_import", {
    request: { generation },
  });
}
export function sessionConnect(
  generation: number,
  profileId: string,
  credential?: CredentialInput,
) {
  return invoke<SessionInfo>("session_connect", {
    request: { generation, profileId, credential },
  });
}
export function sessionsList(generation: number) {
  return invoke<SessionInfo[]>("sessions_list", { request: { generation } });
}
export function confirmHost(
  generation: number,
  sessionId: string,
  challengeId: string,
  accept: boolean,
  revision = "",
) {
  return invoke<void>("session_confirm_host", {
    request: { generation, sessionId, challengeId, accept, revision },
  });
}
export function disconnect(generation: number, sessionId: string) {
  return invoke<void>("session_disconnect", {
    request: { generation, sessionId },
  });
}
export function closeSession(generation: number, sessionId: string) {
  return invoke<void>("session_close", { request: { generation, sessionId } });
}
export function terminalAction(
  generation: number,
  sessionId: string,
  terminalId: string,
  command: TerminalCommand,
) {
  return invoke<void>("terminal_action", {
    request: { generation, sessionId, terminalId, command },
  });
}
export function terminalOpen(
  generation: number,
  sessionId: string,
  cols: number,
  rows: number,
  channel: Channel<TerminalEvent>,
) {
  return invoke<string>("terminal_open", {
    request: { generation, sessionId, cols, rows },
    channel,
  });
}
export function clipboardWrite(generation: number, text: string) {
  return invoke<void>("clipboard_write", { request: { generation, text } });
}
export function clipboardRead(generation: number) {
  return invoke<string>("clipboard_read", { request: { generation } });
}

export interface ProfileImportResult {
  imported: number;
  skipped: number;
  foldersImported: number;
}
export const profilesImport = (generation: number) =>
  invoke<ProfileImportResult | null>("profiles_import", {
    request: { generation },
  });
export const profilesExport = (generation: number) =>
  invoke<number | null>("profiles_export", { request: { generation } });
