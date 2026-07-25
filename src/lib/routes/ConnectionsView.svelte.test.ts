// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import { get } from "svelte/store";
import type { AppSettings, ForwardConfig, TunnelGroup } from "../types";
import { forwards, statusById, statsById, lastErrorById } from "../stores/forwards";
import { settings } from "../stores/settings";
import { groups, activeTag } from "../stores/groups";

vi.mock("../ipc", () => ({
  // Consumed by ConnectionsView + its child tree; only deleteGroup is exercised.
  deleteForward: vi.fn(() => Promise.resolve()),
  deleteGroup: vi.fn(() => Promise.resolve()),
  duplicateForward: vi.fn(() => Promise.resolve()),
  copySshCommand: vi.fn(() => Promise.resolve("ssh ...")),
  reorderForwards: vi.fn(() => Promise.resolve()),
  updateGroup: vi.fn(() => Promise.resolve({})),
  createGroup: vi.fn(() => Promise.resolve({})),
  assignForwardGroup: vi.fn(() => Promise.resolve()),
  startGroup: vi.fn(() => Promise.resolve()),
  stopGroup: vi.fn(() => Promise.resolve()),
  connectForward: vi.fn(() => Promise.resolve()),
  disconnectForward: vi.fn(() => Promise.resolve()),
  retryForward: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));

import ConnectionsView from "./ConnectionsView.svelte";
import { deleteGroup, deleteForward, assignForwardGroup } from "../ipc";

const SETTINGS: AppSettings = {
  launchAtLogin: false,
  showNotifications: true,
  themeMode: "system",
  oledMode: false,
  autoReconnect: true,
  autoReconnectDelaySec: 5,
  autoReconnectMaxRetries: 10,
  showInDock: false,
  autoCheckUpdates: true,
  lastSkippedVersion: null,
};

const GROUPS: TunnelGroup[] = [
  { id: "g1", name: "Production", color: null, order: 0, collapsed: false },
];

function mk(id: string, name: string, groupId: string | null): ForwardConfig {
  return {
    id,
    name,
    sshHost: "bastion.example.com",
    sshPort: 22,
    sshUsername: "deploy",
    identityFilePath: null,
    hasStoredPassword: true,
    localBindAddress: "127.0.0.1",
    localPort: 5432,
    remoteHost: "10.0.4.12",
    remotePort: 5432,
    keepAliveIntervalSec: 30,
    keepAliveMaxCount: 5,
    groupId,
    tags: [],
  };
}

const FORWARDS = [mk("a", "Alpha", "g1"), mk("b", "Bravo", "g1"), mk("c", "Charlie", null)];

describe("ConnectionsView — delete group keeps its tunnels", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    settings.set(SETTINGS);
    forwards.set(FORWARDS);
    groups.set(GROUPS);
    activeTag.set(null);
    statusById.set({ a: "disconnected", b: "disconnected", c: "disconnected" });
    statsById.set({});
    lastErrorById.set({});
  });

  it("dispatches deleteGroup only — never deleteForward — and keeps tunnels in the store", async () => {
    render(ConnectionsView);

    // Open the Production group header ⋯ menu → Delete group…
    await fireEvent.click(
      screen.getByRole("button", { name: /production group actions/i }),
    );
    await fireEvent.click(
      screen.getByRole("menuitem", { name: /delete group/i }),
    );

    // Confirm dialog explains members fall to Ungrouped (nothing is deleted).
    expect(screen.getByText(/move to ungrouped/i)).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Delete group" }));

    expect(deleteGroup).toHaveBeenCalledWith("g1");
    // Deleting a group must NOT delete or reassign its member tunnels — the
    // backend clears groupId; the FE issues a single delete_group call.
    expect(deleteForward).not.toHaveBeenCalled();
    expect(assignForwardGroup).not.toHaveBeenCalled();
    // Members are still present (they'll fall to Ungrouped on the backend event).
    expect(get(forwards)).toHaveLength(3);
  });
});
