// @vitest-environment jsdom
/**
 * SettingsView in web mode (spec 02 §10): desktop-only groups disappear, the
 * Server group replaces Startup, backups go through download/upload, and the
 * PATH-shim command is never called.
 */
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import type { AppSettings } from "../types";
import { settings } from "../stores/settings";
import { importMode } from "../stores/backup";

vi.mock("../runtime", () => ({ isWeb: true }));

vi.mock("../ipc", () => ({
  updateSettings: vi.fn((s) => Promise.resolve(s)),
  exportBackup: vi.fn(() => Promise.resolve()),
  importBackup: vi.fn(),
  exportBackupText: vi.fn(() => Promise.resolve('{"version":2}')),
  importBackupText: vi.fn(() =>
    Promise.resolve({ imported: 3, skipped: 1, replaced: false }),
  ),
  checkUpdate: vi.fn(),
  installUpdate: vi.fn(),
  skipUpdate: vi.fn(),
  cliShimStatus: vi.fn(),
  installCliShim: vi.fn(),
  uninstallCliShim: vi.fn(),
}));

vi.mock("../auth", async () => {
  const { writable } = await import("svelte/store");
  return {
    serverVersion: writable("2.3.0"),
    signOut: vi.fn(() => Promise.resolve()),
  };
});

import SettingsView from "./SettingsView.svelte";
import {
  cliShimStatus,
  exportBackupText,
  importBackup,
  importBackupText,
} from "../ipc";
import { signOut } from "../auth";

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
  autoInstallCli: true,
  oledMode: false,
};

describe("SettingsView — web mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    settings.set(SETTINGS);
    importMode.set("merge");
  });

  it("hides desktop-only settings and never probes the PATH shim", () => {
    render(SettingsView);
    for (const name of [
      /launch at login/i,
      /show in dock/i,
      /desktop notifications/i,
      /automatically check for updates/i,
    ]) {
      expect(screen.queryByRole("switch", { name })).not.toBeInTheDocument();
    }
    expect(screen.queryByText("Command line")).not.toBeInTheDocument();
    expect(screen.queryByText(/check for updates/i)).not.toBeInTheDocument();
    expect(cliShimStatus).not.toHaveBeenCalled();
    // Shared settings stay.
    expect(
      screen.getByRole("switch", { name: /auto-reconnect/i }),
    ).toBeInTheDocument();
  });

  it("explains the service install and shows the server version", () => {
    render(SettingsView);
    expect(screen.getByText("Server")).toBeInTheDocument();
    expect(
      screen.getByText("tunnel-pilot-cli service install --web"),
    ).toBeInTheDocument();
    expect(screen.getByText("Tunnel Pilot v2.3.0")).toBeInTheDocument();
  });

  it("signs out", async () => {
    render(SettingsView);
    await fireEvent.click(screen.getByRole("button", { name: /sign out/i }));
    expect(signOut).toHaveBeenCalledOnce();
  });

  it("exports by downloading the backup text", async () => {
    const createObjectURL = vi.fn(() => "blob:backup");
    URL.createObjectURL = createObjectURL;
    URL.revokeObjectURL = vi.fn();
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(() => {});

    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /export configuration/i }),
    );
    await vi.waitFor(() => expect(click).toHaveBeenCalledOnce());

    expect(exportBackupText).toHaveBeenCalledOnce();
    const anchor = click.mock.contexts[0] as HTMLAnchorElement;
    expect(anchor.download).toMatch(
      /^tunnel-pilot-.+-\d{4}-\d{2}-\d{2}\.json$/,
    );
    expect(createObjectURL).toHaveBeenCalledOnce();
    click.mockRestore();
  });

  it("imports an uploaded file through importBackupText", async () => {
    const file = new File(['{"version":2}'], "prod.json", {
      type: "application/json",
    });
    const click = vi
      .spyOn(HTMLInputElement.prototype, "click")
      .mockImplementation(function (this: HTMLInputElement) {
        Object.defineProperty(this, "files", { value: [file] });
        this.dispatchEvent(new Event("change"));
      });

    render(SettingsView);
    await fireEvent.click(
      screen.getByRole("button", { name: /import configuration/i }),
    );
    expect(await screen.findByText("prod.json")).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: /^import$/i }));

    await vi.waitFor(() =>
      expect(importBackupText).toHaveBeenCalledWith('{"version":2}', "merge"),
    );
    expect(importBackup).not.toHaveBeenCalled();
    click.mockRestore();
  });
});
