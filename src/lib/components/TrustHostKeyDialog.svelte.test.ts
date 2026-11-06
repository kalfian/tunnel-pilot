// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { get } from "svelte/store";
import type { ForwardConfig, HostKeyMismatch } from "../types";
import { hostKeyMismatchById } from "../stores/forwards";
import { toasts } from "../ui/toast";

vi.mock("../ipc", () => ({
  trustHostKey: vi.fn(),
  retryForward: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));

import { retryForward, trustHostKey } from "../ipc";
import TrustHostKeyDialog from "./TrustHostKeyDialog.svelte";

const FORWARD: ForwardConfig = {
  id: "abc",
  name: "Postgres",
  sshHost: "bastion.example.com",
  sshPort: 2222,
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
  hostKey: {
    algorithm: "ssh-ed25519",
    fingerprint: "SHA256:old",
    pinnedAt: "2026-10-01T09:00:00Z",
  },
};

const MISMATCH: HostKeyMismatch = {
  expected: FORWARD.hostKey,
  presented: {
    algorithm: "ecdsa-sha2-nistp256",
    fingerprint: "SHA256:presentedPresentedPresentedPresentedPres",
    pinnedAt: null,
  },
  source: "pinned",
};

function renderDialog(mismatch: HostKeyMismatch = MISMATCH) {
  const onClose = vi.fn();
  const onTrusted = vi.fn();
  render(TrustHostKeyDialog, {
    props: { forward: FORWARD, mismatch, onClose, onTrusted },
  });
  return { onClose, onTrusted };
}

describe("TrustHostKeyDialog", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    toasts.set([]);
    hostKeyMismatchById.set({ abc: MISMATCH });
    vi.mocked(trustHostKey).mockResolvedValue({
      ...FORWARD,
      hostKey: { ...MISMATCH.presented, pinnedAt: "2026-11-06T10:00:00Z" },
    });
  });

  it("shows the presented key and the verify-first warning, Cancel focused", () => {
    renderDialog();
    expect(
      screen.getByRole("dialog", { name: "Trust the new host key?" }),
    ).toBeInTheDocument();
    expect(
      screen.getByText(MISMATCH.presented.fingerprint),
    ).toBeInTheDocument();
    expect(screen.getByText("ecdsa-sha2-nistp256")).toBeInTheDocument();
    expect(screen.getByText("bastion.example.com:2222")).toBeInTheDocument();
    expect(
      screen.getByText(
        /compare this fingerprint with the one the server's administrator/i,
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/someone may be intercepting the connection/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/replaces the pinned key for this tunnel/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
  });

  it("says known_hosts is left alone for a known_hosts conflict", () => {
    renderDialog({ ...MISMATCH, expected: null, source: "knownHosts" });
    expect(
      screen.getByText(/pins this key for this tunnel and reconnects/i),
    ).toHaveTextContent(/known_hosts is not changed/i);
  });

  it("confirm trusts, then retries — in that order", async () => {
    const order: string[] = [];
    vi.mocked(trustHostKey).mockImplementation(async (id) => {
      order.push(`trust:${id}`);
      return FORWARD;
    });
    vi.mocked(retryForward).mockImplementation(async (id) => {
      order.push(`retry:${id}`);
    });
    const { onTrusted, onClose } = renderDialog();

    await fireEvent.click(
      screen.getByRole("button", { name: "Trust and reconnect" }),
    );

    await waitFor(() => expect(retryForward).toHaveBeenCalledTimes(1));
    expect(order).toEqual(["trust:abc", "retry:abc"]);
    expect(onTrusted).toHaveBeenCalledWith("abc");
    expect(onClose).not.toHaveBeenCalled();
    // The backend cleared the refusal without an event; the mirror follows.
    expect(get(hostKeyMismatchById).abc).toBeNull();
  });

  it("cancel does nothing", async () => {
    const { onClose, onTrusted } = renderDialog();
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(trustHostKey).not.toHaveBeenCalled();
    expect(retryForward).not.toHaveBeenCalled();
    expect(onTrusted).not.toHaveBeenCalled();
    expect(get(hostKeyMismatchById).abc).toEqual(MISMATCH);
  });

  it("Escape cancels without trusting", async () => {
    const { onClose } = renderDialog();
    await fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(trustHostKey).not.toHaveBeenCalled();
  });

  it("a failed trust toasts and never reconnects", async () => {
    vi.mocked(trustHostKey).mockRejectedValue(
      new Error("forward abc has no changed host key waiting to be trusted"),
    );
    const { onTrusted } = renderDialog();
    await fireEvent.click(
      screen.getByRole("button", { name: "Trust and reconnect" }),
    );
    await waitFor(() =>
      expect(get(toasts).map((t) => t.message)).toContain(
        "Couldn't trust the new key: Error: forward abc has no changed host key waiting to be trusted",
      ),
    );
    expect(retryForward).not.toHaveBeenCalled();
    expect(onTrusted).not.toHaveBeenCalled();
    expect(get(hostKeyMismatchById).abc).toEqual(MISMATCH);
  });

  it("a failed reconnect after trusting is reported as such", async () => {
    vi.mocked(retryForward).mockRejectedValue(new Error("port in use"));
    renderDialog();
    await fireEvent.click(
      screen.getByRole("button", { name: "Trust and reconnect" }),
    );
    await waitFor(() =>
      expect(get(toasts).map((t) => t.message)).toContain(
        "Key trusted, but reconnecting failed: Error: port in use",
      ),
    );
  });
});
