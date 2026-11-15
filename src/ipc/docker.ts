import { invoke, Channel } from "@tauri-apps/api/core";
import type { TerminalEvent } from "./contracts";
export type Kind = "container" | "image" | "volume";
export type Action =
  "start" | "stop" | "restart" | "pause" | "unpause" | "remove";
export type ComposeAction = "up" | "start" | "stop" | "restart" | "down";
export interface Resource {
  handle: string;
  kind: Kind;
  id: string;
  name: string;
  state: string;
  image: string;
  ports: string;
  tags: string;
  project: string;
  details: string;
}
export interface Output {
  stdout: string;
  stderr: string;
  exitCode: number;
}
export interface Confirmation {
  token: string;
  server: string;
  name: string;
  action: string;
  needsName: boolean;
}
export interface Project {
  id: string;
  identity: { profileId: string; host: string; port: number; username: string };
  name: string;
  path: string;
}
export interface ComposeConfirmation {
  token: string;
  server: string;
  project: string;
  path: string;
  action: string;
  volumeToken: string | null;
}
export interface DockerEvent {
  operationId: string;
  sessionId: string;
  generation: number;
  sequence: number;
  kind: "stdout" | "stderr" | "completed" | "failed" | "cancelled";
  bytes: number[];
  exitCode: number | null;
  message: string | null;
}
const call = <T>(
  command: string,
  generation: number,
  sessionId: string,
  extra: object = {},
) => invoke<T>(command, { request: { generation, sessionId, ...extra } });
export const inventory = (g: number, s: string, kind: Kind) =>
  call<{ version: string; resources: Resource[] }>("docker_inventory", g, s, {
    kind,
  });
export const inspect = (g: number, s: string, resourceId: string) =>
  call<unknown>("docker_inspect", g, s, { resourceId });
export const prepare = (
  g: number,
  s: string,
  resourceId: string,
  action: Action,
) => call<Confirmation>("docker_prepare", g, s, { resourceId, action });
export const apply = (
  g: number,
  s: string,
  token: string,
  typedName?: string,
) => call<Output>("docker_apply", g, s, { token, typedName });
export const logs = (
  generation: number,
  sessionId: string,
  resourceId: string,
  channel: Channel<DockerEvent>,
) =>
  invoke<string>("docker_logs", {
    request: { generation, sessionId, resourceId },
    channel,
  });
export const exec = (
  generation: number,
  sessionId: string,
  resourceId: string,
  channel: Channel<TerminalEvent>,
) =>
  invoke<string>("docker_exec", {
    request: { generation, sessionId, resourceId },
    channel,
  });
export const streamAction = (
  g: number,
  s: string,
  operationId: string,
  sequence?: number,
) => call<void>("docker_stream_action", g, s, { operationId, sequence });
export const projects = (g: number, s: string) =>
  call<Project[]>("compose_list", g, s);
export const register = (g: number, s: string, path: string) =>
  call<Project>("compose_register", g, s, { path });
export const removeProject = (g: number, s: string, projectId: string) =>
  call<void>("compose_remove", g, s, { projectId });
export const projectStatus = (g: number, s: string, projectId: string) =>
  call<unknown>("compose_status", g, s, { projectId });
export const prepareCompose = (
  g: number,
  s: string,
  projectId: string,
  action: ComposeAction,
  removeVolumes = false,
) =>
  call<ComposeConfirmation>("compose_prepare", g, s, {
    projectId,
    action,
    removeVolumes,
  });
export const applyCompose = (
  generation: number,
  sessionId: string,
  confirmation: ComposeConfirmation,
  typedProject: string | undefined,
  volumeConfirmation: string | undefined,
  channel: Channel<DockerEvent>,
) =>
  invoke<string>("compose_apply", {
    request: {
      generation,
      sessionId,
      token: confirmation.token,
      typedProject,
      volumeToken: confirmation.volumeToken,
      volumeConfirmation,
    },
    channel,
  });
