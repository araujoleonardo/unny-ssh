export interface AppError {
  code: string;
  message: string;
  retryable: boolean;
  correlationId: string;
}
export interface VaultStatus {
  state: "missing" | "locked" | "unlocked" | "lock_failed";
  generation: number;
}
export type VaultRequest =
  | { action: "create" | "unlock"; password: string }
  | { action: "lock" }
  | { action: "change_password"; old_password: string; new_password: string }
  | {
      action: "reset";
      password: string;
      confirmation: string;
      confirmation_token: string;
    };
export interface ResourceContext {
  sessionId: string;
  generation: number;
}
export type OperationState = "running" | "completed" | "cancelled" | "failed";
export interface StreamEvent {
  operationId: string;
  generation: number;
  sequence: number;
  state: OperationState;
  data: string;
}

export interface ProgressSnapshot {
  processed: number;
  total: number | null;
}

export type AuthMode = "password" | "key" | "agent";
export interface ProfileInput {
  id: string | null;
  name: string;
  host: string;
  port: number;
  username: string;
  authMode: AuthMode;
  initialDirectory: string | null;
}
export interface Profile extends ProfileInput {
  hasSavedSecret: boolean;
}
export interface CredentialInput {
  password?: string;
  keyImport?: string;
  passphrase?: string;
}
export interface HostChallenge {
  id: string;
  host: string;
  port: number;
  fingerprint: string;
  previousFingerprint: string | null;
  changed: boolean;
  algorithm: string;
  expiresInSeconds: number;
}
export interface SessionInfo {
  id: string;
  generation: number;
  profile: ProfileInput;
  state: "connecting" | "host_pending" | "connected" | "disconnected" | "error";
  errorCode: string | null;
  errorMessage: string | null;
  challenge: HostChallenge | null;
  sftp: boolean;
}
export interface TerminalEvent {
  terminalId: string;
  sessionId: string;
  generation: number;
  sequence: number;
  kind: "data" | "closed" | "error";
  bytes: number[];
  exitStatus: number | null;
}
export type TerminalCommand =
  | { action: "write"; bytes: number[] }
  | { action: "resize"; cols: number; rows: number }
  | { action: "ack"; sequence: number }
  | { action: "close" };
