// @vitest-environment jsdom
/** Settings → Variables (spec 03 §24): list, add/edit dialog, delete flows. */
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from "@testing-library/svelte";
import type { ForwardConfig, Variable } from "../types";
import { variables } from "../stores/variables";
import { forwards } from "../stores/forwards";
import { keychainUnavailable } from "../stores/settings";

vi.mock("../ipc", () => ({
  saveVariable: vi.fn((input: { name: string; secret: boolean }) =>
    Promise.resolve({
      name: input.name,
      secret: input.secret,
      value: "",
      hasStoredValue: input.secret,
    }),
  ),
  deleteVariable: vi.fn(() => Promise.resolve()),
}));

import VariablesSection from "./VariablesSection.svelte";
import { deleteVariable, saveVariable } from "../ipc";

const VARS: Variable[] = [
  { name: "DB_USER", secret: false, value: "kalfian", hasStoredValue: false },
  { name: "DB_PASS", secret: true, value: "", hasStoredValue: true },
  { name: "API_KEY", secret: true, value: "", hasStoredValue: false },
];

function fwd(over: Partial<ForwardConfig>): ForwardConfig {
  return {
    id: "f1",
    name: "Postgres",
    sshHost: "h",
    sshPort: 22,
    sshUsername: "deploy",
    identityFilePath: null,
    hasStoredPassword: false,
    localBindAddress: "127.0.0.1",
    localPort: 5432,
    remoteHost: "db",
    remotePort: 5432,
    keepAliveIntervalSec: 30,
    keepAliveMaxCount: 5,
    groupId: null,
    tags: [],
    hostKey: null,
    passwordVariable: null,
    ...over,
  };
}

function rowFor(name: string): HTMLElement {
  const list = screen.getByRole("list", { name: "Variables" });
  const item = within(list)
    .getAllByRole("listitem")
    .find((li) => li.textContent?.includes(`{{${name}}}`));
  if (!item) throw new Error(`no row for ${name}`);
  return item;
}

async function type(label: RegExp, value: string): Promise<void> {
  await fireEvent.input(screen.getByLabelText(label), { target: { value } });
}

describe("VariablesSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    variables.set(VARS);
    forwards.set([
      fwd({ id: "a", name: "Web", sshUsername: "{{ DB_USER }}-admin" }),
      fwd({
        id: "b",
        name: "Postgres",
        sshUsername: "{{DB_USER}}",
        passwordVariable: "DB_PASS",
      }),
    ]);
    keychainUnavailable.set(false);
  });

  it("explains what variables are for and that changes apply on reconnect", () => {
    render(VariablesSection);
    expect(
      screen.getByRole("heading", { name: "Variables" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/pick up a change when they/i)).toBeInTheDocument();
  });

  it("lists tokens, plain values, masked secret states and usage", () => {
    render(VariablesSection);
    const user = rowFor("DB_USER");
    expect(user).toHaveTextContent("Plain");
    expect(user).toHaveTextContent("kalfian");
    expect(user).toHaveTextContent("Used by 2 tunnels");

    const pass = rowFor("DB_PASS");
    expect(pass).toHaveTextContent("Secret · stored");
    expect(pass).toHaveTextContent("Used by 1 tunnel");

    const key = rowFor("API_KEY");
    expect(key).toHaveTextContent("Secret · no value yet");
    expect(key).toHaveTextContent("Not used");
  });

  it("shows an empty state when there are no variables", () => {
    variables.set([]);
    render(VariablesSection);
    expect(screen.getByText("No variables yet")).toBeInTheDocument();
    expect(
      screen.queryByRole("list", { name: "Variables" }),
    ).not.toBeInTheDocument();
  });

  it("adds a plain variable", async () => {
    render(VariablesSection);
    await fireEvent.click(
      screen.getByRole("button", { name: /add variable/i }),
    );
    const dialog = screen.getByRole("dialog", { name: "Add variable" });
    await type(/^name$/i, "ENV");
    await type(/^value$/i, "prod");
    await fireEvent.click(
      within(dialog).getByRole("button", { name: "Add variable" }),
    );
    expect(saveVariable).toHaveBeenCalledWith({
      name: "ENV",
      secret: false,
      value: "prod",
    });
    await waitFor(() =>
      expect(screen.queryByRole("dialog")).not.toBeInTheDocument(),
    );
  });

  it("adds a secret through a password field and refuses a case clash", async () => {
    render(VariablesSection);
    await fireEvent.click(
      screen.getByRole("button", { name: /add variable/i }),
    );
    const dialog = screen.getByRole("dialog", { name: "Add variable" });
    await fireEvent.click(
      within(dialog).getByRole("radio", { name: "Secret" }),
    );

    await type(/^name$/i, "db_pass");
    await fireEvent.keyDown(screen.getByLabelText(/^name$/i), { key: "s" });
    expect(
      await within(dialog).findByText(/DB_PASS already exists/),
    ).toBeInTheDocument();

    await type(/^name$/i, "TOKEN");
    const secret = screen.getByLabelText(/secret value/i);
    expect(secret).toHaveAttribute("type", "password");
    await type(/secret value/i, "hunter2");
    await fireEvent.click(
      within(dialog).getByRole("button", { name: "Add variable" }),
    );
    expect(saveVariable).toHaveBeenCalledWith({
      name: "TOKEN",
      secret: true,
      value: "hunter2",
    });
  });

  it("warns that secret values fall back to plaintext without a keychain", async () => {
    keychainUnavailable.set(true);
    render(VariablesSection);
    await fireEvent.click(
      screen.getByRole("button", { name: /add variable/i }),
    );
    await fireEvent.click(screen.getByRole("radio", { name: "Secret" }));
    expect(screen.getByText(/plaintext fallback file/i)).toBeInTheDocument();
  });

  it("edits a secret with a blank value to keep the stored one (null)", async () => {
    render(VariablesSection);
    await fireEvent.click(
      within(rowFor("DB_PASS")).getByRole("button", { name: "Edit DB_PASS" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Edit variable" });
    // The name is the identity — shown, not editable.
    expect(within(dialog).queryByLabelText(/^name$/i)).not.toBeInTheDocument();
    expect(within(dialog).getByText("{{DB_PASS}}")).toBeInTheDocument();
    const field = within(dialog).getByLabelText(/secret value/i);
    expect(field).toHaveAttribute(
      "placeholder",
      expect.stringMatching(/leave blank to keep/i),
    );
    expect(field).toHaveValue("");

    const save = within(dialog).getByRole("button", { name: "Save changes" });
    expect(save).toBeDisabled(); // nothing changed yet
    await type(/secret value/i, "rotated");
    await fireEvent.click(save);
    expect(saveVariable).toHaveBeenCalledWith({
      name: "DB_PASS",
      secret: true,
      value: "rotated",
    });
  });

  it("blocks turning a username variable secret and says why", async () => {
    render(VariablesSection);
    await fireEvent.click(
      within(rowFor("DB_USER")).getByRole("button", { name: "Edit DB_USER" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Edit variable" });
    await fireEvent.click(
      within(dialog).getByRole("radio", { name: "Secret" }),
    );
    await type(/secret value/i, "x");
    expect(within(dialog).getByRole("alert")).toHaveTextContent(
      /username of Web and Postgres/,
    );
    expect(
      within(dialog).getByRole("button", { name: "Save changes" }),
    ).toBeDisabled();
  });

  it("shows the backend's refusal inline and keeps the dialog open", async () => {
    vi.mocked(saveVariable).mockRejectedValueOnce({
      kind: "invalidInput",
      message: "variable ENV contains a control character",
    });
    render(VariablesSection);
    await fireEvent.click(
      screen.getByRole("button", { name: /add variable/i }),
    );
    await type(/^name$/i, "ENV");
    await type(/^value$/i, "prod");
    await fireEvent.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Add variable",
      }),
    );
    expect(
      await screen.findByText("variable ENV contains a control character"),
    ).toBeInTheDocument();
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });

  it("deletes an unused variable after confirming", async () => {
    render(VariablesSection);
    await fireEvent.click(
      within(rowFor("API_KEY")).getByRole("button", { name: "Delete API_KEY" }),
    );
    const dialog = screen.getByRole("dialog", { name: "Delete variable?" });
    // Safe default focus (spec §9).
    expect(
      within(dialog).getByRole("button", { name: "Cancel" }),
    ).toHaveFocus();
    await fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete" }),
    );
    expect(deleteVariable).toHaveBeenCalledWith("API_KEY");
  });

  it("explains an in-use variable instead of offering delete", async () => {
    render(VariablesSection);
    await fireEvent.click(
      within(rowFor("DB_PASS")).getByRole("button", { name: "Delete DB_PASS" }),
    );
    const dialog = screen.getByRole("dialog", { name: "DB_PASS is in use" });
    expect(dialog).toHaveTextContent("Postgres");
    expect(dialog).toHaveTextContent("password");
    expect(
      within(dialog).queryByRole("button", { name: "Delete" }),
    ).not.toBeInTheDocument();
    await fireEvent.click(within(dialog).getByRole("button", { name: "OK" }));
    expect(deleteVariable).not.toHaveBeenCalled();
  });
});
