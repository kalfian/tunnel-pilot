// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from "@testing-library/svelte";
import ForwardForm from "./ForwardForm.svelte";
import type { ForwardConfig } from "../types";
import { forwards } from "../stores/forwards";

vi.mock("../ipc", () => ({
  createForward: vi.fn((input) =>
    Promise.resolve({ id: "new-1", hasStoredPassword: false, ...input }),
  ),
  updateForward: vi.fn((id, input) =>
    Promise.resolve({ id, hasStoredPassword: false, ...input }),
  ),
  setForwardPassword: vi.fn(() => Promise.resolve()),
  clearForwardPassword: vi.fn(() => Promise.resolve()),
  forgetHostKey: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn(() => Promise.resolve("/home/u")),
}));

import { createForward, forgetHostKey, setForwardPassword } from "../ipc";

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
  // At least one auth method is required (F46); the default auth tab is Password.
  await type(/^password$/i, "s3cret");
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

  it("closes a pristine Add form immediately with no discard prompt", async () => {
    const onClose = vi.fn();
    render(ForwardForm, { props: { mode: "add", onClose } });

    await fireEvent.click(
      screen.getByRole("button", { name: /close dialog/i }),
    );

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(
      screen.queryByText(/discard unsaved changes\?/i),
    ).not.toBeInTheDocument();
  });

  it("prompts to discard when closing a dirty form, then closes on confirm", async () => {
    const onClose = vi.fn();
    render(ForwardForm, { props: { mode: "add", onClose } });
    await type(/^name$/i, "Postgres");

    await fireEvent.click(
      screen.getByRole("button", { name: /close dialog/i }),
    );

    // The confirm intercepts the close: onClose is deferred until confirmed.
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByText(/discard unsaved changes\?/i)).toBeInTheDocument();

    await fireEvent.click(screen.getByRole("button", { name: /^discard$/i }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe("ForwardForm — host key pin", () => {
  const PINNED: ForwardConfig = {
    id: "fwd-1",
    name: "Postgres",
    sshHost: "bastion.example.com",
    sshPort: 22,
    sshUsername: "deploy",
    identityFilePath: "~/.ssh/id_ed25519",
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
      fingerprint: "SHA256:pinnedPinnedPinnedPinnedPinnedPinnedPinne",
      pinnedAt: "2026-10-01T09:00:00Z",
    },
  };

  function hostKeyGroup(): HTMLElement {
    return screen.getByRole("group", { name: "Host key" });
  }

  beforeEach(() => {
    vi.clearAllMocks();
    forwards.set([]);
    vi.mocked(forgetHostKey).mockResolvedValue({ ...PINNED, hostKey: null });
  });

  it("is not shown when adding a tunnel", () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    expect(screen.queryByRole("group", { name: "Host key" })).toBeNull();
    expect(screen.queryByText(/not pinned yet/i)).toBeNull();
  });

  it("shows the pinned algorithm, fingerprint and date, read-only", () => {
    render(ForwardForm, {
      props: { mode: "edit", forward: PINNED, onClose: vi.fn() },
    });
    const g = hostKeyGroup();
    expect(within(g).getByText("ssh-ed25519")).toBeInTheDocument();
    expect(
      within(g).getByText(PINNED.hostKey!.fingerprint),
    ).toBeInTheDocument();
    const time = g.querySelector("time");
    expect(time).toHaveAttribute("datetime", "2026-10-01T09:00:00Z");
    expect(within(g).getByText(/pinned on/i)).toBeInTheDocument();
    expect(
      within(g).getByText("Changing the SSH host or port clears the pin."),
    ).toBeInTheDocument();
    // Nothing to type into: the pin is never user input.
    expect(within(g).queryByRole("textbox")).toBeNull();
  });

  it("says when nothing is pinned yet, with no Forget action", () => {
    render(ForwardForm, {
      props: {
        mode: "edit",
        forward: { ...PINNED, hostKey: null },
        onClose: vi.fn(),
      },
    });
    const g = hostKeyGroup();
    expect(
      within(g).getByText(
        "Not pinned yet — pinned on the first successful connection.",
      ),
    ).toBeInTheDocument();
    expect(within(g).queryByRole("button", { name: /forget/i })).toBeNull();
  });

  it("warns that saving clears the pin once host or port is edited", async () => {
    render(ForwardForm, {
      props: { mode: "edit", forward: PINNED, onClose: vi.fn() },
    });
    await type(/^port$/i, "2222");
    expect(
      within(hostKeyGroup()).getByText(
        "You changed the host or port, so saving clears this pin.",
      ),
    ).toBeInTheDocument();
  });

  it("Forget… asks first; Keep key backs out without calling ipc", async () => {
    render(ForwardForm, {
      props: { mode: "edit", forward: PINNED, onClose: vi.fn() },
    });
    await fireEvent.click(
      screen.getByRole("button", {
        name: "Forget pinned host key for Postgres",
      }),
    );
    expect(screen.getByText(/forget this key\?/i)).toBeInTheDocument();
    const keep = screen.getByRole("button", { name: "Keep key" });
    await waitFor(() => expect(keep).toHaveFocus());
    await fireEvent.click(keep);
    expect(forgetHostKey).not.toHaveBeenCalled();
    expect(
      within(hostKeyGroup()).getByText(PINNED.hostKey!.fingerprint),
    ).toBeInTheDocument();
  });

  it("Escape in the forget confirm cancels it, not the whole form", async () => {
    const onClose = vi.fn();
    render(ForwardForm, {
      props: { mode: "edit", forward: PINNED, onClose },
    });
    await fireEvent.click(
      screen.getByRole("button", {
        name: "Forget pinned host key for Postgres",
      }),
    );
    const keep = screen.getByRole("button", { name: "Keep key" });
    await fireEvent.keyDown(keep, { key: "Escape" });
    expect(screen.queryByRole("button", { name: "Keep key" })).toBeNull();
    expect(onClose).not.toHaveBeenCalled();
    expect(forgetHostKey).not.toHaveBeenCalled();
  });

  it("Forget key drops the pin and shows the not-pinned state", async () => {
    render(ForwardForm, {
      props: { mode: "edit", forward: PINNED, onClose: vi.fn() },
    });
    await fireEvent.click(
      screen.getByRole("button", {
        name: "Forget pinned host key for Postgres",
      }),
    );
    await fireEvent.click(
      screen.getByRole("button", { name: "Forget host key for Postgres" }),
    );
    expect(forgetHostKey).toHaveBeenCalledWith("fwd-1");
    expect(
      await screen.findByText(
        "Not pinned yet — pinned on the first successful connection.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(PINNED.hostKey!.fingerprint)).toBeNull();
    await waitFor(() => expect(hostKeyGroup()).toHaveFocus());
  });

  it("shows the forgotten state before the forwards://changed event lands", async () => {
    // The forward is in the live store and the store has NOT caught up yet.
    forwards.set([PINNED]);
    render(ForwardForm, {
      props: { mode: "edit", forward: PINNED, onClose: vi.fn() },
    });
    await fireEvent.click(
      screen.getByRole("button", {
        name: "Forget pinned host key for Postgres",
      }),
    );
    await fireEvent.click(
      screen.getByRole("button", { name: "Forget host key for Postgres" }),
    );
    expect(
      await screen.findByText(
        "Not pinned yet — pinned on the first successful connection.",
      ),
    ).toBeInTheDocument();
    // Then the event arrives and the store agrees.
    forwards.set([{ ...PINNED, hostKey: null }]);
    await waitFor(() =>
      expect(screen.queryByText(PINNED.hostKey!.fingerprint)).toBeNull(),
    );
    expect(screen.getByText(/not pinned yet/i)).toBeInTheDocument();
  });

  it("follows the live store (a pin landing while the form is open)", async () => {
    const unpinned = { ...PINNED, hostKey: null };
    forwards.set([unpinned]);
    render(ForwardForm, {
      props: { mode: "edit", forward: unpinned, onClose: vi.fn() },
    });
    expect(screen.getByText(/not pinned yet/i)).toBeInTheDocument();
    forwards.set([PINNED]);
    expect(
      await screen.findByText(PINNED.hostKey!.fingerprint),
    ).toBeInTheDocument();
  });
});
