// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/svelte";
import { get } from "svelte/store";
import type { AppSettings, UpdateStatus } from "../types";
import { settings } from "../stores/settings";
import { importMode } from "../stores/backup";
import { updateStatus, updateProgress } from "../stores/updater";
import { activeView } from "../ui/view";

// Control the effective theme so the OLED toggle can be enabled (it is disabled
// in light mode). `vi.mock` is hoisted above imports, so the store is built in a
// `vi.hoisted` block (a minimal Svelte-store contract — no outer refs allowed).
const themeMock = vi.hoisted(() => {
  let value: "light" | "dark" = "dark";
  const subs = new Set<(v: "light" | "dark") => void>();
  return {
    effectiveTheme: {
      subscribe(fn: (v: "light" | "dark") => void): () => void {
        subs.add(fn);
        fn(value);
        return () => subs.delete(fn);
      },
      set(v: "light" | "dark"): void {
        value = v;
        subs.forEach((f) => f(value));
      },
    },
  };
});
vi.mock("../ui/theme", () => ({ effectiveTheme: themeMock.effectiveTheme }));

import SettingsView from "./SettingsView.svelte";

vi.mock("../ipc", () => ({
  updateSettings: vi.fn((s) => Promise.resolve(s)),
  exportBackup: vi.fn(() => Promise.resolve()),
  importBackup: vi.fn(() =>
    Promise.resolve({ imported: 2, skipped: 0, replaced: true }),
  ),
  checkUpdate: vi.fn(() =>
    Promise.resolve({
      available: false,
      version: null,
      notes: null,
      skipped: false,
    }),
  ),
  installUpdate: vi.fn(() => Promise.resolve()),
  skipUpdate: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(() => Promise.resolve("/backups/tp.json")),
  save: vi.fn(() => Promise.resolve("/backups/tp.json")),
}));

import {
  importBackup,
  checkUpdate,
  installUpdate,
  skipUpdate,
  updateSettings,
} from "../ipc";

const SETTINGS: AppSettings = {
  launchAtLogin: false,
  showNotifications: true,
  themeMode: "system",
  autoReconnect: true,
  autoReconnectDelaySec: 3,
  autoReconnectMaxRetries: 5,
  showInDock: true,
  autoCheckUpdates: true,
  lastSkippedVersion: null,
  oledMode: false,
};

describe("SettingsView — backup import mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    settings.set(SETTINGS);
    importMode.set("merge");
  });

  it("selecting Replace updates the import-mode store", async () => {
    render(SettingsView);
    const group = screen.getByRole("radiogroup", { name: /import mode/i });
    const replace = within(group).getByRole("radio", { name: "Replace" });
    await fireEvent.click(replace);
    expect(get(importMode)).toBe("replace");
    expect(replace).toHaveAttribute("aria-checked", "true");
  });

  it("imports the picked file using the selected mode", async () => {
    importMode.set("replace");
    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /import configuration/i }),
    );
    // Confirm dialog appears after the file picker resolves.
    const confirm = await screen.findByRole("button", {
      name: /replace & import/i,
    });
    await fireEvent.click(confirm);
    expect(importBackup).toHaveBeenCalledWith("/backups/tp.json", "replace");
  });
});

describe("SettingsView — OLED black toggle (Appearance)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    settings.set(SETTINGS);
    importMode.set("merge");
    themeMock.effectiveTheme.set("dark");
  });

  it("toggling OLED dispatches an updateSettings patch with oledMode", async () => {
    render(SettingsView);
    const toggle = screen.getByRole("switch", { name: /oled black/i });
    expect(toggle).not.toBeDisabled();
    await fireEvent.click(toggle);
    expect(updateSettings).toHaveBeenCalledWith(
      expect.objectContaining({ oledMode: true }),
    );
  });

  it("is disabled in light mode (no effect when not dark)", () => {
    themeMock.effectiveTheme.set("light");
    render(SettingsView);
    expect(screen.getByRole("switch", { name: /oled black/i })).toBeDisabled();
  });
});

const AVAILABLE: UpdateStatus = {
  available: true,
  version: "2.1.0",
  notes: "Fixes and improvements",
  skipped: false,
};

describe("SettingsView — update banner (spec §8)", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    settings.set(SETTINGS);
    importMode.set("merge");
    updateStatus.set(null);
    updateProgress.set(null);
    activeView.set("settings");
  });

  it("stays hidden (idle) when there is no update", () => {
    render(SettingsView);
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByText(/available/i)).not.toBeInTheDocument();
  });

  it("renders the available state with version + notes disclosure", () => {
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    expect(screen.getByText(/version 2\.1\.0 available/i)).toBeInTheDocument();
    // Notes are behind a disclosure, not dumped inline.
    expect(
      screen.getByText("Fixes and improvements"),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /install & restart/i }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /skip this version/i }),
    ).toBeInTheDocument();
  });

  it("hides the banner when the available version is already skipped", () => {
    // Backend only persists lastSkippedVersion + emits settings://changed; the
    // banner must reconcile against the settings store, not wait for a new status.
    settings.set({ ...SETTINGS, lastSkippedVersion: "2.1.0" });
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    expect(
      screen.queryByText(/version 2\.1\.0 available/i),
    ).not.toBeInTheDocument();
  });

  it("renders the error banner from a status.error event as a readable string", () => {
    // check_update now RETURNS Ok(status) with the failure in status.error and
    // emits it on update://status — the banner derives its error from the store.
    updateStatus.set({
      available: false,
      version: null,
      notes: null,
      skipped: false,
      error: "Could not reach the release server",
    });
    render(SettingsView);
    const alert = screen.getByRole("alert");
    expect(
      within(alert).getByText(/could not reach the release server/i),
    ).toBeInTheDocument();
    expect(alert.textContent).not.toContain("[object Object]");
  });

  it("stays idle when a status event carries no error", () => {
    updateStatus.set({
      available: false,
      version: null,
      notes: null,
      skipped: false,
      error: null,
    });
    render(SettingsView);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText(/update failed/i)).not.toBeInTheDocument();
  });

  it("'Install & restart' dispatches installUpdate()", async () => {
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /install & restart/i }),
    );
    expect(installUpdate).toHaveBeenCalledOnce();
  });

  it("'Skip this version' dispatches skipUpdate(version)", async () => {
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /skip this version/i }),
    );
    expect(skipUpdate).toHaveBeenCalledWith("2.1.0");
  });

  it("renders determinate download progress", () => {
    updateProgress.set([50, 100]);
    render(SettingsView);
    expect(screen.getByText(/downloading… 50%/i)).toBeInTheDocument();
    const bar = screen.getByRole("progressbar", {
      name: /update download progress/i,
    });
    expect(bar).toHaveAttribute("aria-valuenow", "50");
  });

  it("renders the indeterminate installing state", () => {
    updateProgress.set([0, null]);
    render(SettingsView);
    expect(screen.getByText(/installing…/i)).toBeInTheDocument();
  });

  it("renders the ready state when the download completes", () => {
    updateProgress.set([100, 100]);
    render(SettingsView);
    expect(screen.getByText(/update ready/i)).toBeInTheDocument();
  });

  it("shows a checking state while a check is in flight", async () => {
    let resolveCheck: (v: UpdateStatus) => void = () => {};
    vi.mocked(checkUpdate).mockReturnValueOnce(
      new Promise<UpdateStatus>((r) => {
        resolveCheck = r;
      }),
    );
    render(SettingsView);
    await fireEvent.click(screen.getByRole("button", { name: /check now/i }));
    expect(screen.getByText(/checking for updates…/i)).toBeInTheDocument();
    resolveCheck({
      available: false,
      version: null,
      notes: null,
      skipped: false,
    });
  });

  it("surfaces an install failure as the error state with a retry", async () => {
    vi.mocked(installUpdate).mockRejectedValueOnce("verify failed");
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /install & restart/i }),
    );
    const alert = await screen.findByRole("alert");
    expect(within(alert).getByText(/update failed/i)).toBeInTheDocument();
    expect(within(alert).getByText(/verify failed/i)).toBeInTheDocument();

    // Retry re-runs the check.
    await fireEvent.click(
      within(alert).getByRole("button", { name: /retry/i }),
    );
    expect(checkUpdate).toHaveBeenCalledOnce();
  });

  it("renders a structured {message} error as a readable string, never [object Object]", async () => {
    // The IPC layer rejects with a serialized Rust AppError (an object), which
    // used to string-coerce to "[object Object]". It must render its message.
    vi.mocked(installUpdate).mockRejectedValueOnce({
      message: "Signature verification failed",
      kind: "Update",
    });
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /install & restart/i }),
    );
    const alert = await screen.findByRole("alert");
    expect(
      within(alert).getByText(/signature verification failed/i),
    ).toBeInTheDocument();
    expect(alert.textContent).not.toContain("[object Object]");
  });

  it("never shows [object Object] for an opaque object error", async () => {
    // Object with no `message` — must fall back to JSON, not "[object Object]".
    vi.mocked(installUpdate).mockRejectedValueOnce({ code: 500 });
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /install & restart/i }),
    );
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).not.toContain("[object Object]");
    expect(within(alert).getByText(/500/)).toBeInTheDocument();
  });

  it("stays idle (no failure banner) when the error carries no message", async () => {
    // A benign/empty rejection (e.g. a silent startup check with no release)
    // must NOT raise a scary red "Update failed" banner.
    vi.mocked(checkUpdate).mockRejectedValueOnce({});
    render(SettingsView);
    await fireEvent.click(screen.getByRole("button", { name: /check now/i }));
    // Let the rejected promise settle.
    await Promise.resolve();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByText(/update failed/i)).not.toBeInTheDocument();
  });

  it("error 'View log' switches to the activity view", async () => {
    vi.mocked(installUpdate).mockRejectedValueOnce("nope");
    updateStatus.set(AVAILABLE);
    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /install & restart/i }),
    );
    const alert = await screen.findByRole("alert");
    await fireEvent.click(
      within(alert).getByRole("button", { name: /view log/i }),
    );
    expect(get(activeView)).toBe("activity");
  });
});
