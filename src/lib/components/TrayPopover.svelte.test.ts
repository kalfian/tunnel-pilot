// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import type { ForwardConfig, ForwardStatus, TunnelGroup } from "../types";
import {
  forwards,
  statusById,
  lastErrorById,
} from "../stores/forwards";
import { groups } from "../stores/groups";
import { updateStatus } from "../stores/updater";
import TrayPopover from "./TrayPopover.svelte";

// hydrateAll is a network/IPC call; the popover triggers it on mount. Stub it.
vi.mock("../hydrate", () => ({ hydrateAll: vi.fn(() => Promise.resolve()) }));
vi.mock("../ipc", () => ({
  connectForward: vi.fn(() => Promise.resolve()),
  disconnectForward: vi.fn(() => Promise.resolve()),
  retryForward: vi.fn(() => Promise.resolve()),
  startAll: vi.fn(() => Promise.resolve()),
  showWindow: vi.fn(() => Promise.resolve()),
  hideTrayPopover: vi.fn(() => Promise.resolve()),
  quitApp: vi.fn(() => Promise.resolve()),
}));

import {
  connectForward,
  disconnectForward,
  retryForward,
  startAll,
  showWindow,
  hideTrayPopover,
  quitApp,
} from "../ipc";

function mk(
  id: string,
  name: string,
  localPort: number,
  groupId: string | null,
): ForwardConfig {
  return {
    id,
    name,
    sshHost: "bastion.example.com",
    sshPort: 22,
    sshUsername: "deploy",
    identityFilePath: null,
    hasStoredPassword: false,
    localBindAddress: "127.0.0.1",
    localPort,
    remoteHost: "10.0.4.12",
    remotePort: localPort,
    keepAliveIntervalSec: 30,
    keepAliveMaxCount: 5,
    groupId,
    tags: [],
  };
}

const GROUPS: TunnelGroup[] = [
  { id: "g1", name: "Production", color: "green", order: 0, collapsed: false },
];
const FORWARDS = [
  mk("a", "Postgres", 5432, "g1"),
  mk("b", "Redis", 6379, "g1"),
  mk("c", "Kibana", 5601, null),
];

function seed(status: Record<string, ForwardStatus> = {}): void {
  forwards.set(FORWARDS);
  groups.set(GROUPS);
  statusById.set(status);
  lastErrorById.set({});
  updateStatus.set({
    available: false,
    version: null,
    notes: null,
    skipped: false,
  });
}

describe("TrayPopover — layout", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seed();
  });

  it("renders group section headers, an Ungrouped section, rows + ports", () => {
    render(TrayPopover);
    expect(screen.getByText("Production")).toBeInTheDocument();
    expect(screen.getByText("Ungrouped")).toBeInTheDocument();
    expect(screen.getByText("Postgres")).toBeInTheDocument();
    expect(screen.getByText(":5432")).toBeInTheDocument();
    expect(screen.getByText("Kibana")).toBeInTheDocument();
    expect(screen.getByText(":5601")).toBeInTheDocument();
  });

  it("shows the empty state when there are no tunnels", () => {
    forwards.set([]);
    groups.set([]);
    render(TrayPopover);
    expect(screen.getByText("No tunnels yet")).toBeInTheDocument();
  });
});

describe("TrayPopover — row actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seed();
  });

  it("connects a disconnected tunnel on click", async () => {
    seed({ a: "disconnected", b: "connected", c: "disconnected" });
    render(TrayPopover);
    await fireEvent.click(screen.getByRole("button", { name: /connect postgres/i }));
    expect(connectForward).toHaveBeenCalledWith("a");
  });

  it("disconnects a connected tunnel on click", async () => {
    seed({ a: "connected", b: "connected", c: "disconnected" });
    render(TrayPopover);
    await fireEvent.click(
      screen.getByRole("button", { name: /disconnect postgres/i }),
    );
    expect(disconnectForward).toHaveBeenCalledWith("a");
  });

  it("retries a tunnel in error on click", async () => {
    seed({ a: "error", b: "connected", c: "disconnected" });
    render(TrayPopover);
    await fireEvent.click(screen.getByRole("button", { name: /retry postgres/i }));
    expect(retryForward).toHaveBeenCalledWith("a");
  });
});

describe("TrayPopover — footer actions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seed();
  });

  it("Start all dispatches startAll", async () => {
    render(TrayPopover);
    await fireEvent.click(screen.getByRole("button", { name: /start all/i }));
    expect(startAll).toHaveBeenCalledOnce();
  });

  it("Settings opens the main window and hides the popover", async () => {
    render(TrayPopover);
    await fireEvent.click(screen.getByRole("button", { name: /settings/i }));
    expect(showWindow).toHaveBeenCalledOnce();
    expect(hideTrayPopover).toHaveBeenCalledOnce();
  });

  it("Quit dispatches quitApp", async () => {
    render(TrayPopover);
    await fireEvent.click(screen.getByRole("button", { name: /quit/i }));
    expect(quitApp).toHaveBeenCalledOnce();
  });

  it("Esc dismisses via hideTrayPopover", async () => {
    render(TrayPopover);
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(hideTrayPopover).toHaveBeenCalledOnce();
  });
});

describe("TrayPopover — update header", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    seed();
  });

  it("is hidden when no update is available", () => {
    render(TrayPopover);
    expect(screen.queryByText(/update available/i)).not.toBeInTheDocument();
  });

  it("shows the version when an update is available and not skipped", () => {
    updateStatus.set({
      available: true,
      version: "v2.0.1",
      notes: null,
      skipped: false,
    });
    render(TrayPopover);
    expect(screen.getByText(/update available · v2\.0\.1/i)).toBeInTheDocument();
  });

  it("stays hidden when the available update was skipped", () => {
    updateStatus.set({
      available: true,
      version: "v2.0.1",
      notes: null,
      skipped: true,
    });
    render(TrayPopover);
    expect(screen.queryByText(/update available/i)).not.toBeInTheDocument();
  });
});
