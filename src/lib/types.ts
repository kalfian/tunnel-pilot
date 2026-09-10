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
  /** Pure-black backgrounds in dark mode (OLED). Only affects dark surfaces. */
  oledMode: boolean;
  autoReconnect: boolean;
  autoReconnectDelaySec: number;
  autoReconnectMaxRetries: number;
  showInDock: boolean;
  autoCheckUpdates: boolean;
  lastSkippedVersion: string | null;
  /** Install the `tunnel-pilot` CLI on PATH at startup when no admin prompt is needed. */
  autoInstallCli: boolean;
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
  // Human-readable failure from a user-initiated check (Rust
  // `UpdateStatus.error: Option<String>`). `check_update` and the tray "Check
  // for Updates" now RETURN Ok(status) with the failure here + emit it via
  // `update://status` instead of throwing. Null/absent for silent startup
  // checks and successful checks, so the banner stays idle for benign cases.
  error?: string | null;
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

// --- CLI on PATH (spec 02 §6.8 / 03 §20) ---

/** Where the `tunnel-pilot` shim can live. Mirrors Rust `ShimTarget`. */
export type ShimTarget = "userLocal" | "usrLocal";

/** What occupies a candidate shim path. Mirrors Rust `ShimState`. */
export type ShimState =
  "absent" | "linkedToCurrent" | "linkedElsewhere" | "notASymlink";

/** One candidate directory's inspection result. */
export interface ShimEntry {
  target: ShimTarget;
  /** Absolute path of the shim (`<dir>/tunnel-pilot`). */
  path: string;
  state: ShimState;
  /** Where the symlink points; `null` unless `state` is a link. */
  linkedPath: string | null;
  /** The directory exists (or can be created) and is writable without elevation. */
  writable: boolean;
  dirExists: boolean;
}

/** Result of `cli_shim_status` / `install_cli_shim` / `uninstall_cli_shim`. */
export interface CliShimStatus {
  /** False on platforms without a symlink shim story (Windows). */
  supported: boolean;
  /** A shim exists at one of the candidate paths. */
  installed: boolean;
  /** Path of the reported shim (first non-absent candidate). */
  path: string | null;
  target: ShimTarget | null;
  /** The reported shim resolves to the running app binary. */
  linksToCurrent: boolean;
  linkedPath: string | null;
  /** The reported shim points somewhere else — stale. */
  linkedElsewhere: boolean;
  /** A real file occupies the reported path; install/uninstall refuse it. */
  conflict: boolean;
  /** Where `installCliShim(undefined)` would install. */
  installTarget: ShimTarget | null;
  installPath: string | null;
  /** Installing at `installTarget` needs an administrator prompt. */
  needsElevation: boolean;
  currentExe: string | null;
  /** The running binary is a dev build under `target/` — install is refused. */
  devBuild: boolean;
  /** Every candidate, in preference order. */
  entries: ShimEntry[];
}
