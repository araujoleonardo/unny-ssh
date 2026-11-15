import { invoke, Channel } from "@tauri-apps/api/core";
import type { AppError, TerminalEvent } from "./contracts";
export interface RemoteEntry {
  id: string;
  name: string;
  path: string;
  kind: "file" | "directory" | "symlink" | "other";
  size: number | null;
  permissions: number | null;
  mtime: number | null;
  escaped: boolean;
}
export interface Listing {
  directory: RemoteEntry;
  parent: RemoteEntry | null;
  entries: RemoteEntry[];
  nextCursor: string | null;
}
export interface Revision {
  size: number;
  mtime: number | null;
  sha256: string;
}
export interface RemoteDocument {
  id: string;
  entry: RemoteEntry;
  text: string;
  revision: Revision;
  bom: boolean;
  lineEndings: string;
}
export type OpenReply =
  | { kind: "document"; document: RemoteDocument }
  | { kind: "restricted"; entry: RemoteEntry; reason: string };
export type SaveReply =
  | { kind: "saved"; revision: Revision }
  | {
      kind: "conflict";
      token: string;
      current: Revision | null;
      missing: boolean;
    }
  | { kind: "non_atomic"; token: string }
  | { kind: "failed"; error: AppError; residue: string | null };
export type Mutation =
  | { action: "create"; parent: string; name: string; directory: boolean }
  | { action: "prepare"; entry: string; name: string | null }
  | { action: "apply"; token: string };
export type MutationReply =
  | { kind: "created"; entry: RemoteEntry }
  | {
      kind: "confirmation";
      token: string;
      entry: RemoteEntry;
      target: string | null;
    }
  | { kind: "done" };
export const listFiles = (
  generation: number,
  sessionId: string,
  directory?: string,
  cursor?: string,
) =>
  invoke<Listing>("files_list", {
    request: { generation, sessionId, directory, cursor },
  });
export const mutateFiles = (
  generation: number,
  sessionId: string,
  command: Mutation,
) =>
  invoke<MutationReply>("files_mutate", {
    request: { generation, sessionId, command },
  });
export const openDocument = (
  generation: number,
  sessionId: string,
  entryId: string,
) =>
  invoke<OpenReply>("document_open", {
    request: { generation, sessionId, entryId },
  });
export const saveDocument = (
  generation: number,
  sessionId: string,
  documentId: string,
  expectedRevision: Revision,
  text: string,
  token?: string,
) =>
  invoke<SaveReply>("document_save", {
    request: {
      generation,
      sessionId,
      documentId,
      expectedRevision,
      text,
      token,
    },
  });
export const releaseFiles = (
  generation: number,
  sessionId: string,
  documentId?: string,
  cursor?: string,
) =>
  invoke<void>("files_release", {
    request: { generation, sessionId, documentId, cursor },
  });
export interface LocalSelection {
  id: string;
  name: string;
  size: number;
}
export interface TransferSnapshot {
  id: string;
  sessionId: string;
  generation: number;
  kind: string;
  state:
    | "queued"
    | "preparing"
    | "running"
    | "waiting_collision"
    | "completed"
    | "failed"
    | "cancelled"
    | "skipped";
  name: string;
  destination: string;
  processed: number;
  total: number | null;
  ignored: string[];
  ignoredCount: number;
  residues: string[];
  message: string | null;
  collision: {
    token: string;
    target: string;
    nonAtomic: boolean;
    local: boolean;
  } | null;
  sequence: number;
}
export type CollisionDecision =
  | { action: "overwrite"; allow_non_atomic: boolean }
  | { action: "rename"; name: string }
  | { action: "skip" };
export type TransferCommand =
  | { action: "cancel" }
  | { action: "ack"; sequence: number }
  | { action: "resolve"; token: string; decision: CollisionDecision };
export const selectUploads = (generation: number, sessionId: string) =>
  invoke<LocalSelection[]>("transfer_select_files", {
    request: { generation, sessionId },
  });
export const selectDestination = (
  generation: number,
  sessionId: string,
  entryId: string,
) =>
  invoke<{ id: string; name: string; exists: boolean } | null>(
    "transfer_select_destination",
    { request: { generation, sessionId, entryId } },
  );
export const upload = (
  generation: number,
  sessionId: string,
  directoryId: string,
  selectionId: string,
  channel: Channel<TransferSnapshot>,
) =>
  invoke<TransferSnapshot>("transfer_upload", {
    request: { generation, sessionId, directoryId, selectionId },
    channel,
  });
export const download = (
  generation: number,
  sessionId: string,
  entryId: string,
  destinationId: string,
  channel: Channel<TransferSnapshot>,
) =>
  invoke<TransferSnapshot>("transfer_download", {
    request: { generation, sessionId, entryId, destinationId },
    channel,
  });
export const transferAction = (
  generation: number,
  sessionId: string,
  operationId: string,
  command: TransferCommand,
) =>
  invoke<void>("transfer_action", {
    request: { generation, sessionId, operationId, command },
  });
export const transferList = (generation: number, sessionId: string) =>
  invoke<TransferSnapshot[]>("transfers_list", {
    request: { generation, sessionId },
  });
export const finishWindowClose = () => invoke<void>("finish_window_close");

export const terminalAtDirectory = (
  generation: number,
  sessionId: string,
  entryId: string,
  channel: Channel<TerminalEvent>,
) =>
  invoke<string>("files_terminal", {
    request: { generation, sessionId, entryId },
    channel,
  });
