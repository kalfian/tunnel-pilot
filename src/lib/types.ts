/**
 * TypeScript types mirroring the Rust models 1:1 (spec 04-DATA-MODEL.md).
 *
 * All Rust structs use `#[serde(rename_all = "camelCase")]`, so the wire format
 * is camelCase and matches these types exactly. Keep Rust models and this file
 * in lockstep — changing one without the other breaks the IPC contract
 * (AGENTS.md §1).
 */

// --- Forwards (spec 04 §1) ---

export interface ForwardConfig {
  id: string;
  name: string;
  sshHost: string;
  sshPort: number; // u16
  sshUsername: string;
  identityFilePath: string | null;
  hasStoredPassword: boolean;
  localBindAddress: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  keepAliveIntervalSec: number;
  keepAliveMaxCount: number;
  groupId: string | null;
  tags: string[];
}

/** Create/update payload — no `id`, no live state, no secret. */
export type ForwardInput = Omit<ForwardConfig, "id" | "hasStoredPassword">;

// --- Groups & tags (spec 04 §2) ---

export interface TunnelGroup {
  id: string;
  name: string;
  color: string | null;
  order: number;
  collapsed: boolean;
}

export interface GroupInput {
  name: string;
  color: string | null;
  collapsed: boolean;
}

// --- Settings (spec 04 §3) ---

export type ThemeMode = "system" | "light" | "dark";

export interface AppSettings {
  launchAtLogin: boolean;
  showNotifications: boolean;
  themeMode: ThemeMode;
  autoReconnect: boolean;
  autoReconnectDelaySec: number;
  autoReconnectMaxRetries: number;
  showInDock: boolean;
  autoCheckUpdates: boolean;
  lastSkippedVersion: string | null;
}

// --- Status & stats (spec 04 §§4,5) ---

export type ForwardStatus =
  "disconnected" | "connecting" | "connected" | "disconnecting" | "error";

export interface TunnelStats {
  activeConnections: number;
  totalBytesUp: number; // safe as JS number up to 2^53 bytes (~9 PB)
  totalBytesDown: number;
  lastPingLatencyMs: number | null;
  connectedSince: string | null; // RFC3339
}

export interface ForwardRuntime {
  status: ForwardStatus;
  stats: TunnelStats;
  lastError: string | null;
}

// --- Logs (spec 04 §6) ---

export type LogLevel = "info" | "warning" | "error";

export interface LogEntry {
  level: LogLevel;
  tunnelName: string | null;
  message: string;
  timestamp: string; // "HH:mm:ss"
}

// --- Updater (spec 04 §7) ---

export interface UpdateStatus {
  available: boolean;
  version: string | null;
  notes: string | null;
  skipped: boolean;
}

// --- App snapshot (spec 04 §8) ---

export interface AppSnapshot {
  forwards: ForwardConfig[];
  groups: TunnelGroup[];
  settings: AppSettings;
  logs: LogEntry[];
  runtimes: [string, ForwardRuntime][];
  update: UpdateStatus;
  keychainAvailable: boolean;
}

// --- Backup (spec 04 §11) ---

export interface BackupFile {
  version: number;
  exportedAt: string | null;
  forwards: ForwardConfig[]; // no passwords; hasStoredPassword = false
  groups: TunnelGroup[]; // [] when importing a v1 backup
}

export type ImportMode = "replace" | "merge";

export interface ImportResult {
  imported: number;
  skipped: number;
  replaced: boolean;
}
