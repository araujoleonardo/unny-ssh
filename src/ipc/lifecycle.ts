import { invoke } from "@tauri-apps/api/core";
import type { VaultStatus } from "./contracts";
import type { RemoteDocument } from "./files";
export interface DirtyBuffer {
  sessionId: string;
  documentId: string;
  text: string;
}
export interface DraftInfo {
  id: string;
  profileId: string;
  server: string;
  name: string;
  path: string;
}
export const workspaceLock = (generation: number, documents: DirtyBuffer[]) =>
  invoke<VaultStatus>("workspace_lock", { request: { generation, documents } });
export const recoverLock = (password: string, discard = false) =>
  invoke<VaultStatus>("workspace_recover", { request: { password, discard } });
export const draftsList = (generation: number) =>
  invoke<DraftInfo[]>("drafts_list", { request: { generation } });
export const draftRemove = (generation: number, draftId: string) =>
  invoke<void>("draft_remove", { request: { generation, draftId } });
export const draftRestore = (
  generation: number,
  sessionId: string,
  draftId: string,
) =>
  invoke<RemoteDocument>("draft_restore", {
    request: { generation, sessionId, draftId },
  });

export const draftRead = (generation: number, draftId: string) =>
  invoke<{ text: string }>("draft_read", { request: { generation, draftId } });
