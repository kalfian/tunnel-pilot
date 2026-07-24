/**
 * Typed IPC wrappers — the contract source of truth (AGENTS.md §1).
 *
 * ONE function per `#[tauri::command]` in spec 02 §6. Components MUST call these
 * wrappers, never `invoke()` with raw command strings. Argument keys are
 * camelCase (Tauri v2 maps them to the snake_case Rust parameters).
 *
 * NOTE (M0): the backend command handlers are still stubs (implemented M3/M4+),
 * so these are not yet called at runtime — they establish the typed surface.
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppSnapshot,
  ForwardConfig,
  ForwardInput,
  ForwardRuntime,
  GroupInput,
  ImportMode,
  ImportResult,
  LogEntry,
  TunnelGroup,
  UpdateStatus,
} from "./types";

// --- Forwards (spec 02 §6.1) ---

export const listForwards = (): Promise<ForwardConfig[]> =>
  invoke("list_forwards");

export const createForward = (input: ForwardInput): Promise<ForwardConfig> =>
  invoke("create_forward", { input });

export const updateForward = (
  id: string,
  input: ForwardInput,
): Promise<ForwardConfig> => invoke("update_forward", { id, input });

export const deleteForward = (id: string): Promise<void> =>
  invoke("delete_forward", { id });

export const duplicateForward = (id: string): Promise<ForwardConfig> =>
  invoke("duplicate_forward", { id });

export const reorderForwards = (orderedIds: string[]): Promise<void> =>
  invoke("reorder_forwards", { orderedIds });

export const connectForward = (id: string): Promise<void> =>
  invoke("connect_forward", { id });

export const disconnectForward = (id: string): Promise<void> =>
  invoke("disconnect_forward", { id });

export const retryForward = (id: string): Promise<void> =>
  invoke("retry_forward", { id });

export const startAll = (): Promise<void> => invoke("start_all");

export const stopAll = (): Promise<void> => invoke("stop_all");

export const getForwardRuntime = (id: string): Promise<ForwardRuntime> =>
  invoke("get_forward_runtime", { id });

export const copySshCommand = (id: string): Promise<string> =>
  invoke("copy_ssh_command", { id });

export const setForwardPassword = (
  id: string,
  password: string,
): Promise<void> => invoke("set_forward_password", { id, password });

export const clearForwardPassword = (id: string): Promise<void> =>
  invoke("clear_forward_password", { id });

// --- Groups & tags (spec 02 §6.2) ---

export const listGroups = (): Promise<TunnelGroup[]> => invoke("list_groups");

export const createGroup = (input: GroupInput): Promise<TunnelGroup> =>
  invoke("create_group", { input });

export const updateGroup = (
  id: string,
  input: GroupInput,
): Promise<TunnelGroup> => invoke("update_group", { id, input });

export const deleteGroup = (id: string): Promise<void> =>
  invoke("delete_group", { id });

export const assignForwardGroup = (
  forwardId: string,
  groupId: string | null,
): Promise<void> => invoke("assign_forward_group", { forwardId, groupId });

export const startGroup = (groupId: string): Promise<void> =>
  invoke("start_group", { groupId });

export const stopGroup = (groupId: string): Promise<void> =>
  invoke("stop_group", { groupId });

export const listTags = (): Promise<string[]> => invoke("list_tags");

// --- Settings (spec 02 §6.3) ---

export const getSettings = (): Promise<AppSettings> => invoke("get_settings");

export const updateSettings = (input: AppSettings): Promise<AppSettings> =>
  invoke("update_settings", { input });

// --- Logs (spec 02 §6.4) ---

export const getLogs = (): Promise<LogEntry[]> => invoke("get_logs");

export const clearLogs = (): Promise<void> => invoke("clear_logs");

export const getLogsText = (): Promise<string> => invoke("get_logs_text");

// --- Backup (spec 02 §6.5) ---

export const exportBackup = (path: string): Promise<void> =>
  invoke("export_backup", { path });

export const importBackup = (
  path: string,
  mode: ImportMode,
): Promise<ImportResult> => invoke("import_backup", { path, mode });

// --- Updater (spec 02 §6.6) ---

export const checkUpdate = (): Promise<UpdateStatus> => invoke("check_update");

export const installUpdate = (): Promise<void> => invoke("install_update");

export const skipUpdate = (version: string): Promise<void> =>
  invoke("skip_update", { version });

// --- App / window (spec 02 §6.7) ---

export const appHydrate = (): Promise<AppSnapshot> => invoke("app_hydrate");

export const showWindow = (): Promise<void> => invoke("show_window");

export const hideWindow = (): Promise<void> => invoke("hide_window");

export const quitApp = (): Promise<void> => invoke("quit_app");
