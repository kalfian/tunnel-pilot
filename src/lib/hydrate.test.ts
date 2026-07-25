import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

// Registry of the callbacks subscribeEvents() hands to each on* helper, so the
// test can fire synthetic events and assert store reconciliation.
const handlers: Record<string, (e: unknown) => void> = {};

vi.mock("./ipc", () => ({
  appHydrate: vi.fn(),
}));

vi.mock("./events", () => {
  const register =
    (name: string) =>
    (cb: (e: unknown) => void): Promise<() => void> => {
      handlers[name] = cb;
      return Promise.resolve(() => {});
    };
  return {
    onTunnelStatus: register("onTunnelStatus"),
    onTunnelStats: register("onTunnelStats"),
    onLogLine: register("onLogLine"),
    onLogCleared: register("onLogCleared"),
    onForwardsChanged: register("onForwardsChanged"),
    onGroupsChanged: register("onGroupsChanged"),
    onSettingsChanged: register("onSettingsChanged"),
    onUpdateStatus: register("onUpdateStatus"),
    onUpdateProgress: register("onUpdateProgress"),
    onWindowFocus: register("onWindowFocus"),
    onTrayOpened: register("onTrayOpened"),
  };
});

import { appHydrate } from "./ipc";
import { hydrateAll, subscribeEvents } from "./hydrate";
import { forwards, statusById, statsById } from "./stores/forwards";
import { groups } from "./stores/groups";
import { logs } from "./stores/logs";
import { keychainUnavailable, settings } from "./stores/settings";
import { updateProgress, updateStatus } from "./stores/updater";
import type { AppSnapshot, AppSettings } from "./types";

const SETTINGS: AppSettings = {
  launchAtLogin: false,
  showNotifications: true,
  themeMode: "system",
  autoReconnect: true,
  autoReconnectDelaySec: 5,
  autoReconnectMaxRetries: 3,
  showInDock: false,
  autoCheckUpdates: true,
  lastSkippedVersion: null,
};

const SNAPSHOT: AppSnapshot = {
  forwards: [
    {
      id: "a",
      name: "web",
      sshHost: "example.com",
      sshPort: 22,
      sshUsername: "user",
      identityFilePath: null,
      hasStoredPassword: false,
      localBindAddress: "127.0.0.1",
      localPort: 8080,
      remoteHost: "localhost",
      remotePort: 80,
      keepAliveIntervalSec: 0,
      keepAliveMaxCount: 0,
      groupId: null,
      tags: [],
    },
  ],
  groups: [{ id: "g1", name: "prod", color: null, order: 0, collapsed: false }],
  settings: SETTINGS,
  logs: [
    { level: "info", tunnelName: null, message: "boot", timestamp: "00:00:00" },
  ],
  runtimes: [
    [
      "a",
      {
        status: "connected",
        stats: {
          activeConnections: 1,
          totalBytesUp: 10,
          totalBytesDown: 20,
          lastPingLatencyMs: 42,
          connectedSince: "2026-07-25T00:00:00Z",
        },
        lastError: null,
      },
    ],
  ],
  update: { available: false, version: null, notes: null, skipped: false },
  keychainAvailable: false,
};

beforeEach(() => {
  forwards.set([]);
  statusById.set({});
  statsById.set({});
  groups.set([]);
  settings.set(null);
  keychainUnavailable.set(false);
  logs.set([]);
  updateStatus.set(null);
  updateProgress.set(null);
  for (const key of Object.keys(handlers)) {
    delete handlers[key];
  }
  vi.mocked(appHydrate).mockReset();
});

describe("hydrateAll", () => {
  it("populates every store from the snapshot", async () => {
    vi.mocked(appHydrate).mockResolvedValue(SNAPSHOT);

    await hydrateAll();

    expect(get(forwards)).toHaveLength(1);
    expect(get(statusById).a).toBe("connected");
    expect(get(statsById).a.lastPingLatencyMs).toBe(42);
    expect(get(groups)).toHaveLength(1);
    expect(get(settings)).toEqual(SETTINGS);
    expect(get(logs).map((e) => e.message)).toEqual(["boot"]);
    expect(get(updateStatus)).toEqual(SNAPSHOT.update);
    // keychainAvailable=false → the UI-facing "unavailable" flag is true.
    expect(get(keychainUnavailable)).toBe(true);
  });
});

describe("subscribeEvents", () => {
  it("wires every event to its store reconcile function", async () => {
    vi.mocked(appHydrate).mockResolvedValue(SNAPSHOT);
    await subscribeEvents();

    handlers.onForwardsChanged({ payload: SNAPSHOT.forwards });
    expect(get(forwards)).toHaveLength(1);

    handlers.onTunnelStatus({
      payload: { id: "a", status: "error", lastError: "boom" },
    });
    expect(get(statusById).a).toBe("error");

    handlers.onTunnelStats({
      payload: {
        id: "a",
        stats: {
          activeConnections: 0,
          totalBytesUp: 5,
          totalBytesDown: 5,
          lastPingLatencyMs: null,
          connectedSince: null,
        },
      },
    });
    expect(get(statsById).a.totalBytesUp).toBe(5);

    handlers.onLogLine({
      payload: {
        level: "warning",
        tunnelName: "web",
        message: "hi",
        timestamp: "00:00:01",
      },
    });
    expect(get(logs)[0].message).toBe("hi");

    handlers.onLogCleared(null);
    expect(get(logs)).toEqual([]);

    handlers.onGroupsChanged({ payload: SNAPSHOT.groups });
    expect(get(groups)).toHaveLength(1);

    handlers.onSettingsChanged({ payload: SETTINGS });
    expect(get(settings)).toEqual(SETTINGS);

    handlers.onUpdateStatus({
      payload: {
        available: true,
        version: "2.1.0",
        notes: null,
        skipped: false,
      },
    });
    expect(get(updateStatus)?.available).toBe(true);

    handlers.onUpdateProgress({ payload: { downloaded: 100, total: 500 } });
    expect(get(updateProgress)).toEqual([100, 500]);
  });

  it("re-hydrates on window://focus", async () => {
    vi.mocked(appHydrate).mockResolvedValue(SNAPSHOT);
    await subscribeEvents();
    expect(vi.mocked(appHydrate)).not.toHaveBeenCalled();

    handlers.onWindowFocus(null);
    await Promise.resolve();
    expect(vi.mocked(appHydrate)).toHaveBeenCalledOnce();
  });

  it("re-hydrates on tray://opened (popover show)", async () => {
    vi.mocked(appHydrate).mockResolvedValue(SNAPSHOT);
    await subscribeEvents();
    expect(vi.mocked(appHydrate)).not.toHaveBeenCalled();

    handlers.onTrayOpened(null);
    await Promise.resolve();
    expect(vi.mocked(appHydrate)).toHaveBeenCalledOnce();
  });
});
