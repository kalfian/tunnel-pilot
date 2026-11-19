// @vitest-environment jsdom
/** Web mode: server-side identity path, and secret-variable server binding. */
import "@testing-library/jest-dom/vitest";
import { beforeEach, describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/svelte";

vi.mock("../runtime", () => ({ isWeb: true }));
vi.mock("../ipc", () => ({
  createForward: vi.fn(),
  updateForward: vi.fn(),
  allowVariableEndpoint: vi.fn(),
  setForwardPassword: vi.fn(),
  clearForwardPassword: vi.fn(),
}));

import ForwardForm from "./ForwardForm.svelte";
import { allowVariableEndpoint, createForward } from "../ipc";
import { variables } from "../stores/variables";
import type { ForwardConfig } from "../types";

function authGroup(): HTMLElement {
  return screen.getByRole("radiogroup", { name: /authentication method/i });
}

async function type(label: RegExp, value: string): Promise<void> {
  await fireEvent.input(screen.getByLabelText(label), { target: { value } });
}

async function fillWithVariable(): Promise<void> {
  await type(/^name$/i, "Postgres");
  await type(/^host$/i, "Bastion.example.com");
  await type(/^username$/i, "deploy");
  await type(/local port/i, "5432");
  await type(/remote host/i, "10.0.4.12");
  await type(/remote port/i, "5432");
  await fireEvent.click(
    within(authGroup()).getByRole("radio", { name: "Variable" }),
  );
  await fireEvent.change(screen.getByLabelText(/password variable/i), {
    target: { value: "DB_PASS" },
  });
}

const save = () =>
  fireEvent.click(screen.getByRole("button", { name: /save tunnel/i }));

describe("ForwardForm — web mode", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(createForward).mockResolvedValue({
      id: "new",
    } as unknown as ForwardConfig);
    variables.set([
      {
        name: "DB_PASS",
        secret: true,
        value: "",
        hasStoredValue: true,
        allowedEndpoints: ["known.example:22"],
      },
    ]);
  });

  it("takes a server path for the identity file, with no Browse button", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fireEvent.click(
      within(authGroup()).getByRole("radio", { name: /identity file/i }),
    );

    const field = screen.getByLabelText(/^identity file$/i);
    expect(field).toHaveAccessibleDescription(/on the server/i);
    expect(
      screen.queryByRole("button", { name: /browse/i }),
    ).not.toBeInTheDocument();
  });

  it("asks for the secret's value before using it on a new server", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fillWithVariable();
    const proof = screen.getByLabelText(/confirm DB_PASS for this server/i);
    expect(proof).toHaveAccessibleDescription(/bastion\.example\.com:22/);

    // Without the value nothing is sent.
    await save();
    expect(createForward).not.toHaveBeenCalled();
    expect(allowVariableEndpoint).not.toHaveBeenCalled();
    expect(screen.getByText(/enter the value of DB_PASS/i)).toBeInTheDocument();

    await fireEvent.input(proof, { target: { value: "s3cret" } });
    await save();
    expect(allowVariableEndpoint).toHaveBeenCalledWith({
      name: "DB_PASS",
      sshHost: "Bastion.example.com",
      sshPort: 22,
      value: "s3cret",
    });
    expect(createForward).toHaveBeenCalledTimes(1);
    const order = [
      vi.mocked(allowVariableEndpoint).mock.invocationCallOrder[0],
      vi.mocked(createForward).mock.invocationCallOrder[0],
    ];
    expect(order[0]).toBeLessThan(order[1]);
  });

  it("asks again only when an edit sends the secret somewhere new", async () => {
    const existing: ForwardConfig = {
      id: "f1",
      name: "Postgres",
      sshHost: "known.example",
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
      hostKey: {
        algorithm: "ssh-ed25519",
        fingerprint: "SHA256:x",
        pinnedAt: null,
      },
      passwordVariable: "DB_PASS",
    };
    render(ForwardForm, {
      props: { mode: "edit", forward: existing, onClose: vi.fn() },
    });
    // Same variable, same allowed server: no prompt.
    expect(
      screen.queryByLabelText(/confirm DB_PASS for this server/i),
    ).not.toBeInTheDocument();
    // Its pin can't be forgotten from the web.
    expect(
      screen.queryByRole("button", { name: /forget pinned host key/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText(/only be forgotten from the desktop app/i),
    ).toBeInTheDocument();
    // The same host spelled differently drops the pin: prompt.
    await type(/^host$/i, "KNOWN.example");
    expect(
      screen.getByLabelText(/confirm DB_PASS for this server/i),
    ).toBeInTheDocument();
    // Another server: prompt.
    await type(/^host$/i, "other.example");
    expect(
      screen.getByLabelText(/confirm DB_PASS for this server/i),
    ).toHaveAccessibleDescription(/other\.example:22/);
  });

  it("does not save the tunnel when the value is wrong", async () => {
    vi.mocked(allowVariableEndpoint).mockRejectedValue({
      kind: "invalidInput",
      message: "that is not the value of DB_PASS",
    });
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fillWithVariable();
    await fireEvent.input(
      screen.getByLabelText(/confirm DB_PASS for this server/i),
      { target: { value: "guess" } },
    );
    await save();
    expect(createForward).not.toHaveBeenCalled();
    expect(
      screen.getByLabelText(/confirm DB_PASS for this server/i),
    ).toHaveValue("");
  });
});
