# 04 — Data Model: Rust structs, TS types, config schema, migration

> The v2 data model as a clean evolution of v1, with a v1→v2 migration importer.
> Every model has a Rust (serde) definition AND a matching TypeScript type.
> Cross-refs: [02-ARCHITECTURE.md](02-ARCHITECTURE.md), [03-TECH-SPEC.md](03-TECH-SPEC.md),
> [06-MIGRATION-REPO.md](06-MIGRATION-REPO.md).

## Conventions

- **Serde casing**: all structs use `#[serde(rename_all = "camelCase")]` so the JSON /
  IPC wire format is camelCase and matches the TypeScript types 1:1.
- **IDs**: `String` (uuid v4).
- **Optional**: Rust `Option<T>` ↔ TS `T | null` (serde emits `null`, not omitted, unless
  a field uses `skip_serializing_if`).
- TS types live in `src/lib/types.ts`; Rust models in `src-tauri/src/state/` (or a shared
  `models.rs`). The `lib/ipc.ts` wrappers are the contract (see [AGENTS.md](AGENTS.md)).

---

## 1. ForwardConfig

Adds `groupId` + `tags` (new in v2) and `hasStoredPassword` (keychain flag). **The
plaintext `sshPassword` field from v1 does NOT exist in the v2 persisted model** — secrets
live in keychain/fallback (see [03 §credentials](03-TECH-SPEC.md#credentials)).

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardConfig {
    pub id: String,                         // uuid v4
    pub name: String,
    pub ssh_host: String,
    pub ssh_port: u16,                      // default 22
    pub ssh_username: String,
    pub identity_file_path: Option<String>, // mutually exclusive with password at auth time
    pub has_stored_password: bool,          // true if a secret exists in keychain/fallback
    pub local_bind_address: String,         // default "127.0.0.1"
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub keep_alive_interval_sec: u32,       // default 30; 0 => treated as 10 at runtime
    pub keep_alive_max_count: u32,          // default 5; 0 => treated as 3 at runtime
    pub group_id: Option<String>,           // NEW v2
    pub tags: Vec<String>,                  // NEW v2 (default empty)
}
```
Derived (not persisted): `needs_password` = `identity_file_path.is_none() && !has_stored_password`.

**Auth precedence (nit b — phrased identically to [03 §1](03-TECH-SPEC.md#ssh)):** identity
file and password are mutually exclusive at auth time; if both are somehow set, **the identity
file takes precedence** (matches v1).

```typescript
export interface ForwardConfig {
  id: string;
  name: string;
  sshHost: string;
  sshPort: number;                 // u16
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
```

**`ForwardInput`** (create/update payload — no `id`, no live state, no secret):
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardInput {
    pub name: String,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_username: String,
    pub identity_file_path: Option<String>,
    pub local_bind_address: String,
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    pub keep_alive_interval_sec: u32,
    pub keep_alive_max_count: u32,
    pub group_id: Option<String>,
    pub tags: Vec<String>,
}
```
```typescript
export type ForwardInput = Omit<ForwardConfig, "id" | "hasStoredPassword">;
```
Passwords are set/cleared only via `set_forward_password` / `clear_forward_password`
(see [02 §6.1](02-ARCHITECTURE.md)).

---

## 2. TunnelGroup (new v2)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelGroup {
    pub id: String,             // uuid v4
    pub name: String,
    pub color: Option<String>,  // hex accent for the folder header (optional)
    pub order: u32,             // explicit group ordering (groups are few; explicit is fine)
    #[serde(default)]
    pub collapsed: bool,        // F13: folder collapse state, persisted per group (05 needs this)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInput { pub name: String, pub color: Option<String>, pub collapsed: bool }
```
```typescript
export interface TunnelGroup { id: string; name: string; color: string | null; order: number; collapsed: boolean; }
export interface GroupInput { name: string; color: string | null; collapsed: boolean; }
```
`update_group` **persists `collapsed`** (toggling a folder open/closed in the UI calls
`update_group` so the state survives restarts — [05-UI-UX-SPEC.md](05-UI-UX-SPEC.md) relies on this).
Tags are free-form strings on `ForwardConfig.tags`; the tag list is derived (union of all
forwards' tags) via `list_tags`. **Grouping model (CONFIRMED / resolved):** a forward has
at most **one exclusive `groupId`** (a folder, typically an environment) **plus many
additive `tags`** — folders are exclusive, tags are additive. Ungrouped forwards
(`groupId == null`) render under a default/"Ungrouped" section.

---

## 3. AppSettings

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub launch_at_login: bool,              // default true
    pub show_notifications: bool,           // default true
    pub theme_mode: ThemeMode,              // default System
    pub auto_reconnect: bool,               // default true
    pub auto_reconnect_delay_sec: u32,      // default 5
    pub auto_reconnect_max_retries: u32,    // default 3
    pub show_in_dock: bool,                 // default false
    pub auto_check_updates: bool,           // default true
    pub last_skipped_version: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode { System, Light, Dark }
```
```typescript
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
```

---

## 4. ForwardStatus (5-state)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardStatus { Disconnected, Connecting, Connected, Disconnecting, Error }
```
```typescript
export type ForwardStatus =
  | "disconnected" | "connecting" | "connected" | "disconnecting" | "error";
```
`disconnecting` is a real transient state (clicks ignored while in it). `error` allows retry.

---

## 5. TunnelStats

Wire model (snapshot emitted on `tunnel://stats`); the Rust *runtime* uses atomics
(`StatsInner`, see [03 §stats](03-TECH-SPEC.md#stats)) and converts to this for IPC.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelStats {
    pub active_connections: usize,
    pub total_bytes_up: u64,
    pub total_bytes_down: u64,
    pub last_ping_latency_ms: Option<u64>, // None if never pinged
    pub connected_since: Option<String>,   // RFC3339; None if not connected
}
```
```typescript
export interface TunnelStats {
  activeConnections: number;
  totalBytesUp: number;               // may exceed 2^53 only for huge transfers; use number, document
  totalBytesDown: number;
  lastPingLatencyMs: number | null;
  connectedSince: string | null;      // RFC3339
}
```
Derived `uptime` computed on the frontend from `connectedSince` at render time.
Note: byte counters as JS `number` are safe up to 2^53 bytes (~9 PB) — acceptable.

**`ForwardRuntime`** (returned by `get_forward_runtime`, and in `AppSnapshot`):
```rust
#[serde(rename_all = "camelCase")]
pub struct ForwardRuntime { pub status: ForwardStatus, pub stats: TunnelStats, pub last_error: Option<String> }
```
```typescript
export interface ForwardRuntime { status: ForwardStatus; stats: TunnelStats; lastError: string | null; }
```

---

## 6. LogEntry

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub level: LogLevel,
    pub tunnel_name: Option<String>,   // None for app-level logs
    pub message: String,
    pub timestamp: String,             // formatted "HH:mm:ss"
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel { Info, Warning, Error }
```
```typescript
export type LogLevel = "info" | "warning" | "error";
export interface LogEntry {
  level: LogLevel;
  tunnelName: string | null;
  message: string;
  timestamp: string;                   // "HH:mm:ss"
}
```
Formatted line (for Copy All / `get_logs_text`): `[HH:mm:ss] [LEVEL] [tunnel] message`
(omit `[tunnel]` when `tunnelName` is null). Not persisted; cap 500 newest-first.

---

## 7. UpdateStatus

```rust
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub available: bool,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub skipped: bool,   // version == last_skipped_version
}
```
```typescript
export interface UpdateStatus { available: boolean; version: string | null; notes: string | null; skipped: boolean; }
```

---

## 8. AppSnapshot (returned by `app_hydrate`)

```rust
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub forwards: Vec<ForwardConfig>,
    pub groups: Vec<TunnelGroup>,
    pub settings: AppSettings,
    pub logs: Vec<LogEntry>,
    pub runtimes: Vec<(String, ForwardRuntime)>, // (forwardId, runtime)
    pub update: UpdateStatus,
    pub keychain_available: bool,
}
```
```typescript
export interface AppSnapshot {
  forwards: ForwardConfig[];
  groups: TunnelGroup[];
  settings: AppSettings;
  logs: LogEntry[];
  runtimes: [string, ForwardRuntime][];
  update: UpdateStatus;
  keychainAvailable: boolean;
}
```

---

## 9. Config file JSON schema (v2)

File: `tunnel_pilot_config.json` in app-support/config dir. **Order of `forwards` array =
display order** (no explicit order field on forwards — mirrors v1). Groups DO have explicit
`order`. Passwords are NOT in this file.

```json
{
  "schemaVersion": 2,
  "forwards": [
    {
      "id": "3f2a...uuid",
      "name": "Prod DB",
      "sshHost": "bastion.example.com",
      "sshPort": 22,
      "sshUsername": "deploy",
      "identityFilePath": "/Users/me/.ssh/id_ed25519",
      "hasStoredPassword": false,
      "localBindAddress": "127.0.0.1",
      "localPort": 5432,
      "remoteHost": "db.internal",
      "remotePort": 5432,
      "keepAliveIntervalSec": 30,
      "keepAliveMaxCount": 5,
      "groupId": "grp-prod",
      "tags": ["prod", "database"]
    }
  ],
  "groups": [
    { "id": "grp-prod", "name": "Production", "color": "#EF4444", "order": 0, "collapsed": false }
  ],
  "settings": {
    "launchAtLogin": true,
    "showNotifications": true,
    "themeMode": "system",
    "autoReconnect": true,
    "autoReconnectDelaySec": 5,
    "autoReconnectMaxRetries": 3,
    "showInDock": false,
    "autoCheckUpdates": true,
    "lastSkippedVersion": null
  }
}
```

`forwards`, `groups`, `settings` saved independently via read-merge-write of the full file
(see [03 §persistence](03-TECH-SPEC.md#persistence)). `schemaVersion` gates migration.

---

<a id="keychain"></a>
## 10. Keychain key scheme + fallback store

- **Service** (`KC_SERVICE`): the stable string **`"tunnel-pilot"`** — deliberately chosen,
  **need NOT match any bundle id**, and must stay constant forever so keychain entries survive
  any future bundle-id change (nit c). Do not derive it from the bundle id.
- **Account**: the forward `id` (uuid). One secret per forward.
- API in [03 §credentials](03-TECH-SPEC.md#credentials).

**Fallback secrets file** (only used when keychain unavailable) — kept **separate** from
`tunnel_pilot_config.json` so backup-strip and config-merge logic stay clean:

File: `tunnel_pilot_secrets.json` (app-support dir), `0600` perms where supported:
```json
{ "schemaVersion": 1, "secrets": { "<forwardId>": "<plaintext-password>" } }
```
When this file is in use, the UI shows a persistent warning (driven by
`keychain_available=false` in `AppSnapshot`). This file is **never** included in backups.

---

## 11. Backup format (v2)

Export strips passwords by design and never touches keychain/fallback secrets. Import
validates each entry, rejects `version > current`, and REPLACES (default) or MERGES
(optional UX polish, see [01 N7](01-PRD.md)).

```rust
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub version: u32,                 // backup format version; v2 = 2
    #[serde(default)]
    pub exported_at: Option<String>,  // RFC3339; tolerate absent
    pub forwards: Vec<ForwardConfig>, // hasStoredPassword forced false on export; no secrets
    #[serde(default)]                 // F19: v1 backups have NO `groups` key
    pub groups: Vec<TunnelGroup>,     // v2: include groups so org survives backup
}
```
```typescript
export interface BackupFile {
  version: number;
  exportedAt: string | null;
  forwards: ForwardConfig[];        // no passwords; hasStoredPassword = false
  groups: TunnelGroup[];            // [] when importing a v1 backup
}
export type ImportMode = "replace" | "merge";
export interface ImportResult { imported: number; skipped: number; replaced: boolean; }
```
**Lenient import of v1 backups (F19 — verified against `toJsonForBackup`):** a v1 export is
`{ "version": 1, "exportedAt": "...", "forwards": [...] }` with **no `groups`** and each
forward may carry a legacy `sshPassword` key (v1's main-config `toJson` includes it; the
backup path strips it, but be defensive). Therefore:
- `ForwardConfig` deserialization must tolerate missing v2 fields — apply `#[serde(default)]`
  to `has_stored_password`, `group_id`, `tags`, `keep_alive_*` (defaults per §13). Use
  `#[serde(default)]` on the struct or per-field so a v1 entry parses.
- **Ignore any legacy `sshPassword`** in a backup entry (do not import it as a secret; backups
  are password-free by design). If present, drop it silently.
- `groups` missing ⇒ `[]` (the `#[serde(default)]` above).

On export: set every `hasStoredPassword` to `false` (recipient has no secret). On import:
- Reject if `backup.version > 2` (current) with a clear error (matches v1 which rejected `> 1`).
- **Replace**: clear existing forwards+groups, insert backup's (new uuids optional; keep as-is).
- **Merge**: append forwards whose `id` (or name+host+port key) is not already present;
  skip duplicates → reflected in `ImportResult.skipped`.
- Imported forwards start with `hasStoredPassword=false` (user must re-enter passwords).

---

## 12. v1 → v2 migration

> **F2 — CRITICAL. The v1 config lives at an OS path that Tauri's dir resolution will NOT
> reproduce**, so relying on `app_config_dir` alone silently loses every user's config on
> upgrade. Migration MUST probe the **hardcoded, verified v1 paths** directly.

### Verified v1 facts (read from the Flutter source)
- **Bundle id / package name differ.** macOS bundle id = **`com.kalfian.tunnelpilot`** (NO
  underscore — `macos/Runner/Configs/AppInfo.xcconfig`). The Dart package name
  `com.kalfian.tunnel_pilot` (WITH underscore) is a different string and is **not** the
  macOS app-support folder name.
- **Config filename**: `tunnel_pilot_config.json` (`lib/services/storage_service.dart`).
- **v1 config paths** (Flutter `path_provider` `getApplicationSupportDirectory()`):
  - **macOS**: `~/Library/Application Support/com.kalfian.tunnelpilot/tunnel_pilot_config.json`
  - **Windows**: `%APPDATA%\kalfian\Tunnel Pilot\tunnel_pilot_config.json`
    (path_provider uses `%APPDATA%\<CompanyName>\<ProductName>`; CompanyName=`kalfian`,
    ProductName=`Tunnel Pilot` per `windows/runner/Runner.rc`).
  - **Linux**: **N/A — v1 never shipped a Linux release** (F17). No v1 config to import;
    Linux is fresh-install only.
- **v1 main config includes plaintext `sshPassword`** in each forward (`toJson`), so migration
  is where secrets move into the keychain.

### Detection & path probe (`storage/migration.rs`)
1. Determine the canonical v2 config path (`app_config_dir`, [03 §7](03-TECH-SPEC.md#persistence)).
2. If a v2 config already exists AND its `schemaVersion >= 2` → nothing to do.
3. **If the v2 location has no config**, probe the hardcoded v1 path for the current OS:
```rust
fn v1_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    { Some(home()?.join("Library/Application Support/com.kalfian.tunnelpilot/tunnel_pilot_config.json")) }
    #[cfg(target_os = "windows")]
    { Some(appdata()?.join(r"kalfian\Tunnel Pilot\tunnel_pilot_config.json")) }
    #[cfg(target_os = "linux")]
    { None } // v1 never shipped on Linux (F17)
}
```
   Also treat a config found *at the v2 path* but missing `schemaVersion`/`< 2` as v1 (covers
   same-dir cases). If a v1 file is found → import it, then write the result to the **v2** path.

### Import steps
1. Parse v1 JSON leniently (`#[serde(default)]` for all v2-only fields; see §11).
2. Write a `.v1-backup` copy of the original v1 file (in place, next to the source).
3. For each forward:
   - Map v1 fields → v2 `ForwardConfig`; `groupId=null`, `tags=[]`, `hasStoredPassword` per below.
   - If v1 `sshPassword` is a non-empty string: `credentials::set_password(id, pw)`; set
     `has_stored_password=true`; **do not** carry the plaintext into the v2 file. If keychain
     unavailable → write to the fallback secrets file, still `has_stored_password=true`, set
     the warning flag.
   - If no password: `has_stored_password=false`.
4. Map v1 `settings` → v2 `AppSettings` (all fields carried; `lastSkippedVersion` preserved if present).
5. Set `schemaVersion=2`, `groups=[]`; write atomically to the **v2** path.
6. Idempotent: once the v2 file has `schemaVersion==2`, migration is skipped on subsequent boots.

### Acceptance
- [ ] On macOS/Windows, a v1 config at the **hardcoded v1 path** is detected even though the
      v2 `app_config_dir` folder name differs, and imported without data loss.
- [ ] v1 plaintext passwords land in keychain (or fallback+warning); no plaintext in the v2 file.
- [ ] `.v1-backup` written; re-run is a no-op once `schemaVersion==2`.
- [ ] On Linux, no v1 probe occurs (fresh install); no crash from the absent path.

---

## 13. Defaults reference

| Field | Default |
|-------|---------|
| `ForwardConfig.sshPort` | 22 |
| `ForwardConfig.localBindAddress` | "127.0.0.1" |
| `ForwardConfig.keepAliveIntervalSec` | 30 (0→10 at runtime) |
| `ForwardConfig.keepAliveMaxCount` | 5 (0→3 at runtime) |
| `ForwardConfig.tags` | `[]` |
| `AppSettings.launchAtLogin` | true |
| `AppSettings.showNotifications` | true |
| `AppSettings.themeMode` | system |
| `AppSettings.autoReconnect` | true |
| `AppSettings.autoReconnectDelaySec` | 5 |
| `AppSettings.autoReconnectMaxRetries` | 3 |
| `AppSettings.showInDock` | false |
| `AppSettings.autoCheckUpdates` | true |
| `schemaVersion` (config) | 2 |
| `version` (backup) | 2 |
