// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import ForwardForm from "./ForwardForm.svelte";

vi.mock("../ipc", () => ({
  createForward: vi.fn((input) =>
    Promise.resolve({ id: "new-1", hasStoredPassword: false, ...input }),
  ),
  updateForward: vi.fn((id, input) =>
    Promise.resolve({ id, hasStoredPassword: false, ...input }),
  ),
  setForwardPassword: vi.fn(() => Promise.resolve()),
  clearForwardPassword: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/home/u")),
}));

import { createForward, setForwardPassword } from "../ipc";

function saveButton(): HTMLButtonElement {
  return screen.getByRole("button", { name: /save tunnel/i });
}

async function type(label: RegExp, value: string): Promise<void> {
  const el = screen.getByLabelText(label);
  await fireEvent.input(el, { target: { value } });
}

async function fillValid(): Promise<void> {
  await type(/^name$/i, "Postgres");
  await type(/^host$/i, "bastion.example.com");
  await type(/^username$/i, "deploy");
  await type(/local port/i, "5432");
  await type(/remote host/i, "10.0.4.12");
  await type(/remote port/i, "5432");
}

describe("ForwardForm", () => {
  beforeEach(() => vi.clearAllMocks());

  it("gates submit: Save is disabled until the form is valid", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    expect(saveButton()).toBeDisabled();
    await fillValid();
    expect(saveButton()).toBeEnabled();
  });

  it("shows field errors on an invalid submit and does not call ipc", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    // Force a submit attempt while invalid via the hidden submit button.
    const form = document.querySelector("form")!;
    await fireEvent.submit(form);
    expect(await screen.findByText("Name is required.")).toBeInTheDocument();
    expect(createForward).not.toHaveBeenCalled();
  });

  it("creates the forward and stores a typed password on valid save", async () => {
    const onClose = vi.fn();
    render(ForwardForm, { props: { mode: "add", onClose } });
    await fillValid();
    await type(/^password$/i, "s3cret");
    await fireEvent.click(saveButton());

    expect(createForward).toHaveBeenCalledTimes(1);
    const arg = vi.mocked(createForward).mock.calls[0][0];
    expect(arg).toMatchObject({
      name: "Postgres",
      sshHost: "bastion.example.com",
      sshUsername: "deploy",
      localPort: 5432,
      remotePort: 5432,
      sshPort: 22,
    });
    // Password never rides in ForwardInput — it goes through the secret channel.
    expect(arg).not.toHaveProperty("password");
    expect(setForwardPassword).toHaveBeenCalledWith("new-1", "s3cret");
  });
});
