// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/svelte";
import { get } from "svelte/store";
import type { AppSettings } from "../types";
import { settings } from "../stores/settings";
import { importMode } from "../stores/backup";
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
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(() => Promise.resolve("/backups/tp.json")),
  save: vi.fn(() => Promise.resolve("/backups/tp.json")),
}));

import { importBackup } from "../ipc";

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
