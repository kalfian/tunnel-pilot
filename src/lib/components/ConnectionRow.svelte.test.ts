// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import type {
  ForwardConfig,
  ForwardStatus,
  HostKeyMismatch,
  TunnelStats,
} from "../types";
import ConnectionRow from "./ConnectionRow.svelte";

// Components test against the IPC contract, not a live backend (AGENTS §6).
vi.mock("../ipc", () => ({
  connectForward: vi.fn(() => Promise.resolve()),
  disconnectForward: vi.fn(() => Promise.resolve()),
  retryForward: vi.fn(() => Promise.resolve()),
  duplicateForward: vi.fn(() => Promise.resolve()),
  copySshCommand: vi.fn(() => Promise.resolve("ssh ...")),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));

import { connectForward, disconnectForward, retryForward } from "../ipc";

const FORWARD: ForwardConfig = {
  id: "abc",
  name: "Postgres",
  sshHost: "bastion.example.com",
  sshPort: 22,
  sshUsername: "deploy",
  identityFilePath: null,
  hasStoredPassword: false,
  localBindAddress: "127.0.0.1",
  localPort: 5432,
  remoteHost: "10.0.4.12",
  remotePort: 5432,
  keepAliveIntervalSec: 30,
  keepAliveMaxCount: 5,
  groupId: null,
  tags: [],
  hostKey: null,
};

const STATS: TunnelStats = {
  activeConnections: 3,
  totalBytesUp: 12_400_000,
  totalBytesDown: 88_100_000,
  lastPingLatencyMs: 41,
  connectedSince: new Date().toISOString(),
};

function renderRow(status: ForwardStatus, lastError: string | null = null) {
  return render(ConnectionRow, {
    props: {
      forward: FORWARD,
      status,
      stats: status === "connected" ? STATS : ({} as TunnelStats),
      lastError,
      selected: false,
      onSelect: vi.fn(),
      onEdit: vi.fn(),
      onDelete: vi.fn(),
      onViewLog: vi.fn(),
    },
  });
}

describe("ConnectionRow", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders the name and mono route", () => {
    renderRow("disconnected");
    expect(screen.getByText("Postgres")).toBeInTheDocument();
    expect(
      screen.getByText("127.0.0.1:5432 → 10.0.4.12:5432"),
    ).toBeInTheDocument();
  });

  it("connects when toggled on from disconnected", async () => {
    renderRow("disconnected");
    const toggle = screen.getByRole("switch", { name: /connect postgres/i });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await fireEvent.click(toggle);
    expect(connectForward).toHaveBeenCalledWith("abc");
    expect(disconnectForward).not.toHaveBeenCalled();
  });

  it("disconnects when toggled off from connected, and shows stat chips", async () => {
    renderRow("connected");
    const toggle = screen.getByRole("switch", { name: /disconnect postgres/i });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    // Live stats surface only when connected.
    expect(screen.getByLabelText(/active connections/i)).toBeInTheDocument();
    await fireEvent.click(toggle);
    expect(disconnectForward).toHaveBeenCalledWith("abc");
  });

  it("shows a connecting subtitle and disables re-toggle while pending", () => {
    renderRow("connecting");
    expect(screen.getByText("Connecting…")).toBeInTheDocument();
    const toggle = screen.getByRole("switch");
    expect(toggle).toBeDisabled();
  });

  it("surfaces the error strip with a working Retry action", async () => {
    renderRow("error", "Auth failed: permission denied");
    expect(
      screen.getByText("Auth failed: permission denied"),
    ).toBeInTheDocument();
    await fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retryForward).toHaveBeenCalledWith("abc");
  });

  describe("host key changed", () => {
    const PINNED: HostKeyMismatch = {
      expected: {
        algorithm: "ssh-ed25519",
        fingerprint: "SHA256:oldOldOldOldOldOldOldOldOldOldOldOldOldOld",
        pinnedAt: "2026-10-01T09:00:00Z",
      },
      presented: {
        algorithm: "ssh-ed25519",
        fingerprint: "SHA256:newNewNewNewNewNewNewNewNewNewNewNewNewNew",
        pinnedAt: null,
      },
      source: "pinned",
    };

    function renderMismatch(
      mismatch: HostKeyMismatch,
      status: ForwardStatus = "error",
    ) {
      const onTrustHostKey = vi.fn();
      const onViewLog = vi.fn();
      render(ConnectionRow, {
        props: {
          forward: FORWARD,
          status,
          stats: {} as TunnelStats,
          lastError: "host key changed for bastion.example.com:22 …",
          hostKeyMismatch: mismatch,
          selected: false,
          onSelect: vi.fn(),
          onEdit: vi.fn(),
          onDelete: vi.fn(),
          onViewLog,
          onTrustHostKey,
        },
      });
      return { onTrustHostKey, onViewLog };
    }

    it("replaces the error strip with both fingerprints and no Retry", () => {
      renderMismatch(PINNED);
      const region = screen.getByRole("region", {
        name: /host key changed for postgres/i,
      });
      expect(region).toBeInTheDocument();
      expect(screen.getByRole("alert")).toHaveTextContent(
        /different key than the one pinned/i,
      );
      expect(screen.getByText("Pinned")).toBeInTheDocument();
      expect(screen.getByText("Server now presents")).toBeInTheDocument();
      expect(
        screen.getByText(PINNED.expected!.fingerprint),
      ).toBeInTheDocument();
      expect(
        screen.getByText(PINNED.presented.fingerprint),
      ).toBeInTheDocument();
      expect(screen.getAllByText("ssh-ed25519")).toHaveLength(2);
      // Never auto-retried, so no Retry and no generic message.
      expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
      expect(screen.queryByText(/host key changed for bastion/i)).toBeNull();
    });

    it("explains a known_hosts conflict without a pinned fingerprint", () => {
      renderMismatch({ ...PINNED, expected: null, source: "knownHosts" });
      expect(screen.getByRole("alert")).toHaveTextContent(
        /conflicts with your ~\/\.ssh\/known_hosts, which Tunnel Pilot never modifies/i,
      );
      expect(screen.getByText("known_hosts entry")).toBeInTheDocument();
      expect(
        screen.getByText(PINNED.presented.fingerprint),
      ).toBeInTheDocument();
    });

    it("labels its actions with the tunnel name and hands them up", async () => {
      const { onTrustHostKey, onViewLog } = renderMismatch(PINNED);
      const trust = screen.getByRole("button", {
        name: "Trust new key for Postgres",
      });
      expect(trust).toHaveTextContent("Trust new key…");
      await fireEvent.click(trust);
      expect(onTrustHostKey).toHaveBeenCalledTimes(1);
      await fireEvent.click(
        screen.getByRole("button", { name: "View log for Postgres" }),
      );
      expect(onViewLog).toHaveBeenCalledTimes(1);
      expect(
        screen.getByRole("button", {
          name: "Copy the fingerprint Postgres now presents",
        }),
      ).toBeInTheDocument();
    });

    it("is hidden once the tunnel leaves the error state", () => {
      renderMismatch(PINNED, "disconnected");
      expect(screen.queryByText("Server now presents")).toBeNull();
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });
});
