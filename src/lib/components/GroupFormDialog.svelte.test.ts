// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import type { TunnelGroup } from "../types";
import GroupFormDialog from "./GroupFormDialog.svelte";

// Components test against the IPC contract, not a live backend (AGENTS §6).
vi.mock("../ipc", () => ({
  createGroup: vi.fn(() => Promise.resolve({})),
  updateGroup: vi.fn(() => Promise.resolve({})),
}));

import { createGroup, updateGroup } from "../ipc";

const GROUP: TunnelGroup = {
  id: "g1",
  name: "Production",
  color: "blue",
  order: 0,
  collapsed: false,
};

describe("GroupFormDialog — create", () => {
  beforeEach(() => vi.clearAllMocks());

  it("creates a group with the typed name and picked color", async () => {
    render(GroupFormDialog, { props: { mode: "add", onClose: vi.fn() } });

    const name = screen.getByLabelText("Name");
    await fireEvent.input(name, { target: { value: "Staging" } });
    await fireEvent.click(screen.getByRole("radio", { name: "Green" }));
    await fireEvent.click(
      screen.getByRole("button", { name: /create group/i }),
    );

    expect(createGroup).toHaveBeenCalledWith({
      name: "Staging",
      color: "green",
      collapsed: false,
    });
  });

  it("keeps Create disabled until a name is entered", async () => {
    render(GroupFormDialog, { props: { mode: "add", onClose: vi.fn() } });
    const save = screen.getByRole("button", { name: /create group/i });
    expect(save).toBeDisabled();
    await fireEvent.input(screen.getByLabelText("Name"), {
      target: { value: "X" },
    });
    expect(save).toBeEnabled();
  });
});

describe("GroupFormDialog — edit", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renames a group via update_group, preserving collapsed", async () => {
    render(GroupFormDialog, {
      props: { mode: "edit", group: GROUP, onClose: vi.fn() },
    });

    const name = screen.getByLabelText("Name");
    expect(name).toHaveValue("Production");
    await fireEvent.input(name, { target: { value: "Prod" } });
    await fireEvent.click(screen.getByRole("button", { name: /save changes/i }));

    expect(updateGroup).toHaveBeenCalledWith("g1", {
      name: "Prod",
      color: "blue",
      collapsed: false,
    });
  });

  it("keeps Save disabled until something changes", () => {
    render(GroupFormDialog, {
      props: { mode: "edit", group: GROUP, onClose: vi.fn() },
    });
    expect(screen.getByRole("button", { name: /save changes/i })).toBeDisabled();
  });
});
