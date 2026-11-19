// @vitest-environment jsdom
/** Web mode: trusting a new key for a tunnel that sends a secret variable. */
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import type { ForwardConfig, HostKeyMismatch } from "../types";

vi.mock("../runtime", () => ({ isWeb: true }));
vi.mock("../ipc", () => ({
  allowVariableEndpoint: vi.fn(() => Promise.resolve({})),
  trustHostKey: vi.fn(() => Promise.resolve({})),
  retryForward: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));

import { allowVariableEndpoint, trustHostKey } from "../ipc";
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
    pinnedAt: null,
  },
  passwordVariable: "DB_PASS",
};

const MISMATCH: HostKeyMismatch = {
  expected: FORWARD.hostKey,
  presented: {
    algorithm: "ssh-ed25519",
    fingerprint: "SHA256:new",
    pinnedAt: null,
  },
  source: "pinned",
};

describe("TrustHostKeyDialog — web, secret variable", () => {
  beforeEach(() => vi.clearAllMocks());

  it("needs the secret's value before trusting, and proves it first", async () => {
    render(TrustHostKeyDialog, {
      props: { forward: FORWARD, mismatch: MISMATCH, onClose: vi.fn() },
    });
    const trust = screen.getByRole("button", { name: /trust and reconnect/i });
    await fireEvent.click(trust);
    expect(trustHostKey).not.toHaveBeenCalled();
    expect(screen.getByText(/enter the value of DB_PASS/i)).toBeInTheDocument();

    await fireEvent.input(screen.getByLabelText(/value of DB_PASS/i), {
      target: { value: "s3cret" },
    });
    await fireEvent.click(trust);
    await waitFor(() => expect(trustHostKey).toHaveBeenCalledWith("abc"));
    expect(allowVariableEndpoint).toHaveBeenCalledWith({
      name: "DB_PASS",
      sshHost: "bastion.example.com",
      sshPort: 2222,
      value: "s3cret",
    });
    expect(
      vi.mocked(allowVariableEndpoint).mock.invocationCallOrder[0],
    ).toBeLessThan(vi.mocked(trustHostKey).mock.invocationCallOrder[0]);
  });
});
