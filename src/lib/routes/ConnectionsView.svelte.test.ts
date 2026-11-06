// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { get } from "svelte/store";
import type {
  AppSettings,
  ForwardConfig,
  HostKeyMismatch,
  TunnelGroup,
} from "../types";
import {
  forwards,
  statusById,
  statsById,
  lastErrorById,
  hostKeyMismatchById,
} from "../stores/forwards";
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
  trustHostKey: vi.fn(() => Promise.resolve({})),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));

import ConnectionsView from "./ConnectionsView.svelte";
import {
  deleteGroup,
  deleteForward,
  assignForwardGroup,
  retryForward,
  trustHostKey,
} from "../ipc";

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
  autoInstallCli: true,
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
    hostKey: null,
  };
}

const FORWARDS = [
  mk("a", "Alpha", "g1"),
  mk("b", "Bravo", "g1"),
  mk("c", "Charlie", null),
];

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
    hostKeyMismatchById.set({});
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

describe("ConnectionsView — trust a changed host key", () => {
  const MISMATCH: HostKeyMismatch = {
    expected: {
      algorithm: "ssh-ed25519",
      fingerprint: "SHA256:pinnedPinnedPinnedPinnedPinnedPinnedPinne",
      pinnedAt: "2026-10-01T09:00:00Z",
    },
    presented: {
      algorithm: "ssh-ed25519",
      fingerprint: "SHA256:presentedPresentedPresentedPresentedPres",
      pinnedAt: null,
    },
    source: "pinned",
  };

  beforeEach(() => {
    vi.clearAllMocks();
    settings.set(SETTINGS);
    forwards.set(FORWARDS);
    groups.set(GROUPS);
    activeTag.set(null);
    statusById.set({ a: "error", b: "disconnected", c: "disconnected" });
    statsById.set({});
    lastErrorById.set({ a: "host key changed for bastion.example.com:22 …" });
    hostKeyMismatchById.set({ a: MISMATCH, b: null, c: null });
  });

  it("row → confirm → trust then retry, then the dialog closes and focus returns to the row", async () => {
    const order: string[] = [];
    vi.mocked(trustHostKey).mockImplementation(async (id) => {
      order.push(`trust:${id}`);
      return FORWARDS[0];
    });
    vi.mocked(retryForward).mockImplementation(async (id) => {
      order.push(`retry:${id}`);
    });
    render(ConnectionsView);

    await fireEvent.click(
      screen.getByRole("button", { name: "Trust new key for Alpha" }),
    );
    const dialog = screen.getByRole("dialog", {
      name: "Trust the new host key?",
    });
    expect(dialog).toHaveTextContent(MISMATCH.presented.fingerprint);
    expect(trustHostKey).not.toHaveBeenCalled();

    await fireEvent.click(
      screen.getByRole("button", { name: "Trust and reconnect" }),
    );
    await waitFor(() => expect(order).toEqual(["trust:a", "retry:a"]));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Trust the new host key?" }),
      ).toBeNull(),
    );
    expect(get(hostKeyMismatchById).a).toBeNull();
    await waitFor(() =>
      expect(document.activeElement).toBe(
        document.querySelector('[data-testid="row-body"][data-row-id="a"]'),
      ),
    );
  });

  it("cancel leaves the pin and the refusal untouched", async () => {
    render(ConnectionsView);
    await fireEvent.click(
      screen.getByRole("button", { name: "Trust new key for Alpha" }),
    );
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(
      screen.queryByRole("dialog", { name: "Trust the new host key?" }),
    ).toBeNull();
    expect(trustHostKey).not.toHaveBeenCalled();
    expect(retryForward).not.toHaveBeenCalled();
    expect(get(hostKeyMismatchById).a).toEqual(MISMATCH);
  });

  it("closes the dialog if the refusal goes away underneath it", async () => {
    render(ConnectionsView);
    await fireEvent.click(
      screen.getByRole("button", { name: "Trust new key for Alpha" }),
    );
    expect(
      screen.getByRole("dialog", { name: "Trust the new host key?" }),
    ).toBeInTheDocument();
    // e.g. reconnected from the CLI: a new attempt clears the refusal.
    statusById.update((m) => ({ ...m, a: "connecting" }));
    hostKeyMismatchById.update((m) => ({ ...m, a: null }));
    await waitFor(() =>
      expect(
        screen.queryByRole("dialog", { name: "Trust the new host key?" }),
      ).toBeNull(),
    );
    expect(trustHostKey).not.toHaveBeenCalled();
  });
});
