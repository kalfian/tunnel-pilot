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
import { variables } from "../stores/variables";

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

import {
  clearForwardPassword,
  createForward,
  forgetHostKey,
  setForwardPassword,
  updateForward,
} from "../ipc";

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
    passwordVariable: null,
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

describe("ForwardForm — variables (spec 03 §24)", () => {
  const EXISTING: ForwardConfig = {
    id: "fwd-2",
    name: "Reports",
    sshHost: "bastion.example.com",
    sshPort: 22,
    sshUsername: "{{DB_USER}}",
    identityFilePath: null,
    hasStoredPassword: false,
    localBindAddress: "127.0.0.1",
    localPort: 5433,
    remoteHost: "10.0.4.13",
    remotePort: 5432,
    keepAliveIntervalSec: 30,
    keepAliveMaxCount: 5,
    groupId: null,
    tags: [],
    hostKey: null,
    passwordVariable: "DB_PASS",
  };

  function authGroup(): HTMLElement {
    return screen.getByRole("radiogroup", { name: /authentication method/i });
  }

  beforeEach(() => {
    vi.clearAllMocks();
    forwards.set([]);
    variables.set([
      {
        name: "DB_USER",
        secret: false,
        value: "kalfian",
        hasStoredValue: false,
        allowedEndpoints: [],
      },
      {
        name: "DB_PASS",
        secret: true,
        value: "",
        hasStoredValue: true,
        allowedEndpoints: [],
      },
      {
        name: "NEW_PASS",
        secret: true,
        value: "",
        hasStoredValue: false,
        allowedEndpoints: [],
      },
    ]);
  });

  it("saves a password variable: passwordVariable set, no identity, no password", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fillValid();
    await fireEvent.click(
      within(authGroup()).getByRole("radio", { name: "Variable" }),
    );
    // Variable mode needs a choice before it is valid.
    expect(saveButton()).toBeDisabled();
    const select = screen.getByLabelText(/password variable/i);
    expect(
      within(select).getByRole("option", { name: /NEW_PASS — no value yet/ }),
    ).toBeInTheDocument();
    await fireEvent.change(select, { target: { value: "DB_PASS" } });
    await fireEvent.click(saveButton());

    const arg = vi.mocked(createForward).mock.calls[0][0];
    expect(arg.passwordVariable).toBe("DB_PASS");
    expect(arg.identityFilePath).toBeNull();
    expect(setForwardPassword).not.toHaveBeenCalled();
  });

  it("never asks for a secret's value on the desktop (servers bind implicitly)", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fillValid();
    await fireEvent.click(
      within(authGroup()).getByRole("radio", { name: "Variable" }),
    );
    await fireEvent.change(screen.getByLabelText(/password variable/i), {
      target: { value: "DB_PASS" },
    });
    expect(
      screen.queryByLabelText(/confirm DB_PASS for this server/i),
    ).not.toBeInTheDocument();
  });

  it("sends passwordVariable: null outside variable mode", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fillValid();
    await fireEvent.click(saveButton());
    expect(
      vi.mocked(createForward).mock.calls[0][0].passwordVariable,
    ).toBeNull();
  });

  it("warns when the chosen secret has no value yet", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fireEvent.click(
      within(authGroup()).getByRole("radio", { name: "Variable" }),
    );
    await fireEvent.change(screen.getByLabelText(/password variable/i), {
      target: { value: "NEW_PASS" },
    });
    expect(screen.getByText(/NEW_PASS has no value yet/)).toBeInTheDocument();
  });

  it("points to Settings when there are no secret variables", async () => {
    variables.set([]);
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await fireEvent.click(
      within(authGroup()).getByRole("radio", { name: "Variable" }),
    );
    expect(screen.getByText(/no secret variables yet/i)).toHaveTextContent(
      /Settings → Variables/,
    );
    expect(
      screen.queryByRole("combobox", { name: /password variable/i }),
    ).not.toBeInTheDocument();
  });

  it("opens an existing forward in variable mode and tracks the choice as dirty", async () => {
    render(ForwardForm, {
      props: { mode: "edit", forward: EXISTING, onClose: vi.fn() },
    });
    expect(
      within(authGroup()).getByRole("radio", { name: "Variable" }),
    ).toHaveAttribute("aria-checked", "true");
    const select = screen.getByLabelText(/password variable/i);
    expect(select).toHaveValue("DB_PASS");
    const save = screen.getByRole("button", { name: /save changes/i });
    expect(save).toBeDisabled();
    await fireEvent.change(select, { target: { value: "NEW_PASS" } });
    expect(save).toBeEnabled();
    await fireEvent.click(save);
    expect(vi.mocked(updateForward).mock.calls[0][1]).toMatchObject({
      passwordVariable: "NEW_PASS",
      identityFilePath: null,
    });
  });

  it("clears the tunnel's own stored password when switching to a variable", async () => {
    const withPassword = {
      ...EXISTING,
      passwordVariable: null,
      hasStoredPassword: true,
    };
    render(ForwardForm, {
      props: { mode: "edit", forward: withPassword, onClose: vi.fn() },
    });
    await fireEvent.click(
      within(authGroup()).getByRole("radio", { name: "Variable" }),
    );
    expect(
      screen.getByText(/saving deletes the password/i),
    ).toBeInTheDocument();
    await fireEvent.change(screen.getByLabelText(/password variable/i), {
      target: { value: "DB_PASS" },
    });
    await fireEvent.click(
      screen.getByRole("button", { name: /save changes/i }),
    );
    await waitFor(() =>
      expect(clearForwardPassword).toHaveBeenCalledWith("fwd-2"),
    );
  });

  it("previews the resolved username", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    await type(/^username$/i, "{{ DB_USER }}-admin");
    expect(screen.getByText("kalfian-admin")).toBeInTheDocument();
    expect(screen.getByLabelText(/^username$/i)).toHaveAccessibleDescription(
      /logs in as kalfian-admin/i,
    );
  });

  it("flags unknown and secret variables in the username", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    const field = screen.getByLabelText(/^username$/i);
    await fireEvent.keyDown(field, { key: "{" });
    await type(/^username$/i, "{{NOPE}}");
    expect(
      await screen.findByText(/\{\{NOPE\}\} isn't a variable/),
    ).toBeInTheDocument();
    await type(/^username$/i, "{{DB_PASS}}");
    expect(
      await screen.findByText(/a username can only use plain variables/),
    ).toBeInTheDocument();
    await type(/^username$/i, "{{DB_USER");
    expect(await screen.findByText(/unclosed/i)).toBeInTheDocument();
  });

  it("inserts a plain variable at the caret from its chip", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    const field = screen.getByLabelText(/^username$/i) as HTMLInputElement;
    await type(/^username$/i, "-admin");
    field.setSelectionRange(0, 0);
    // Secret variables are never offered for a username.
    expect(
      screen.queryByRole("button", { name: /insert \{\{DB_PASS\}\}/i }),
    ).not.toBeInTheDocument();
    await fireEvent.click(
      screen.getByRole("button", { name: /insert \{\{DB_USER\}\}/i }),
    );
    expect(field).toHaveValue("{{DB_USER}}-admin");
    await waitFor(() => expect(field).toHaveFocus());
  });
});
