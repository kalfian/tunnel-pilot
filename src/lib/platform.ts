/**
 * Host affordances that differ between the desktop app and web mode (spec 02
 * §10): clipboard, backup files and the identity-file picker. Components call
 * these instead of importing `@tauri-apps/plugin-*` directly, so the web bundle
 * never loads Tauri code. Tauri plugins are imported lazily on the desktop path.
 *
 * (`ui/platform.ts` is a different concern: which OS draws the window chrome.)
 */

import { isWeb } from "./runtime";
import {
  exportBackup,
  exportBackupText,
  importBackup,
  importBackupText,
} from "./ipc";
import type { ImportMode, ImportResult } from "./types";

/** A Tunnel Pilot backup is a few KB; anything this big is the wrong file. */
const MAX_BACKUP_BYTES = 5 * 1024 * 1024;

// --- Clipboard ---

/**
 * Copy text to the clipboard. Web mode is often served over plain HTTP on a
 * LAN address, which is not a secure context, so `navigator.clipboard` is
 * missing there; fall back to a hidden textarea + `execCommand("copy")`.
 */
export async function copyText(text: string): Promise<void> {
  if (!isWeb) {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
    return;
  }
  if (window.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text);
      return;
    } catch {
      // Permission denied or no focus: try the legacy path before failing.
    }
  }
  legacyCopy(text);
}

function legacyCopy(text: string): void {
  const previous =
    document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null;
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.setAttribute("aria-hidden", "true");
  area.tabIndex = -1;
  // On-screen but invisible: some browsers refuse to copy from off-screen
  // or display:none fields.
  Object.assign(area.style, {
    position: "fixed",
    top: "0",
    left: "0",
    width: "1px",
    height: "1px",
    opacity: "0",
    pointerEvents: "none",
  });
  document.body.appendChild(area);
  let copied = false;
  try {
    area.focus({ preventScroll: true });
    area.select();
    area.setSelectionRange(0, text.length);
    copied = document.execCommand("copy");
  } catch {
    copied = false;
  } finally {
    area.remove();
    previous?.focus({ preventScroll: true });
  }
  if (!copied) {
    throw new Error("the browser blocked clipboard access");
  }
}

// --- Backup ---

/**
 * Export the configuration. Desktop: native save dialog, the backend writes
 * the file. Web: download the backup the server returns as text.
 * Resolves `false` when the user cancelled the save dialog.
 */
export async function exportConfiguration(): Promise<boolean> {
  if (isWeb) {
    const text = await exportBackupText();
    downloadText(text, webBackupFileName());
    return true;
  }
  const { save } = await import("@tauri-apps/plugin-dialog");
  const path = await save({
    defaultPath: "tunnel-pilot-backup.json",
    filters: [{ name: "JSON", extensions: ["json"] }],
  });
  if (!path) return false;
  await exportBackup(path);
  return true;
}

/** A chosen backup, waiting for the user to confirm the import. */
export interface PickedBackup {
  /** Desktop: the full path. Web: the uploaded file's name. */
  label: string;
  apply(mode: ImportMode): Promise<ImportResult>;
}

/**
 * Let the user choose a backup to import. Desktop: native open dialog (the
 * backend reads the path). Web: a file input; the text is uploaded on apply.
 * Resolves `null` when the user cancelled.
 */
export async function pickBackup(): Promise<PickedBackup | null> {
  if (isWeb) {
    const file = await chooseFile(".json,application/json");
    if (!file) return null;
    if (file.size > MAX_BACKUP_BYTES) {
      throw new Error(`${file.name} is too large to be a Tunnel Pilot backup`);
    }
    const content = await file.text();
    return {
      label: file.name,
      apply: (mode) => importBackupText(content, mode),
    };
  }
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({
    multiple: false,
    directory: false,
    filters: [{ name: "JSON", extensions: ["json"] }],
    title: "Select a backup file",
  });
  if (typeof picked !== "string") return null;
  return { label: picked, apply: (mode) => importBackup(picked, mode) };
}

/**
 * Desktop only: native picker for an SSH identity file, opened in `~/.ssh`.
 * Web mode has no picker — the key lives on the server, so the form takes a
 * typed server path instead.
 */
export async function pickIdentityFile(): Promise<string | null> {
  const [{ open }, { homeDir }] = await Promise.all([
    import("@tauri-apps/plugin-dialog"),
    import("@tauri-apps/api/path"),
  ]);
  const home = await homeDir();
  const picked = await open({
    multiple: false,
    directory: false,
    defaultPath: `${home}/.ssh`,
    title: "Select identity file",
  });
  return typeof picked === "string" ? picked : null;
}

function webBackupFileName(): string {
  const host = location.hostname.replace(/[^a-z0-9.-]+/gi, "-") || "server";
  const day = new Date().toISOString().slice(0, 10);
  return `tunnel-pilot-${host}-${day}.json`;
}

function downloadText(text: string, fileName: string): void {
  const url = URL.createObjectURL(
    new Blob([text], { type: "application/json" }),
  );
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.rel = "noopener";
  link.style.display = "none";
  document.body.appendChild(link);
  link.click();
  link.remove();
  // Revoke after the click has handed the blob to the download manager.
  setTimeout(() => URL.revokeObjectURL(url), 1_000);
}

function chooseFile(accept: string): Promise<File | null> {
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = accept;
    input.style.display = "none";
    const finish = (file: File | null): void => {
      input.remove();
      resolve(file);
    };
    input.addEventListener("change", () => finish(input.files?.[0] ?? null), {
      once: true,
    });
    input.addEventListener("cancel", () => finish(null), { once: true });
    document.body.appendChild(input);
    input.click();
  });
}

// --- Transport security ---

/**
 * True when web mode runs over plain HTTP from a non-loopback host: traffic,
 * including the admin password, crosses the network unencrypted.
 */
export function isInsecureRemote(
  loc: Pick<Location, "protocol" | "hostname"> = location,
): boolean {
  if (!isWeb || loc.protocol !== "http:") return false;
  return !isLoopbackHost(loc.hostname);
}

export function isLoopbackHost(hostname: string): boolean {
  const host = hostname.toLowerCase();
  return (
    host === "localhost" ||
    host.endsWith(".localhost") ||
    host === "[::1]" ||
    host === "::1" ||
    /^127(\.\d{1,3}){3}$/.test(host)
  );
}
