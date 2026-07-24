// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import type { ForwardConfig } from "../types";
import CommandPalette from "./CommandPalette.svelte";
import { forwards, statusById } from "../stores/forwards";
import { groups } from "../stores/groups";
import { paletteQuery } from "../stores/palette";

// Components test against the IPC contract, not a live backend (AGENTS §6).
vi.mock("../ipc", () => ({
  connectForward: vi.fn(() => Promise.resolve()),
  disconnectForward: vi.fn(() => Promise.resolve()),
  retryForward: vi.fn(() => Promise.resolve()),
  duplicateForward: vi.fn(() => Promise.resolve()),
  copySshCommand: vi.fn(() => Promise.resolve("ssh ...")),
  startAll: vi.fn(() => Promise.resolve()),
  stopAll: vi.fn(() => Promise.resolve()),
  startGroup: vi.fn(() => Promise.resolve()),
  stopGroup: vi.fn(() => Promise.resolve()),
  checkUpdate: vi.fn(() => Promise.resolve({ available: false })),
  updateSettings: vi.fn(() => Promise.resolve()),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));

import { connectForward, disconnectForward, startAll } from "../ipc";

function mk(id: string, name: string): ForwardConfig {
  return {
    id,
    name,
    sshHost: "bastion.example.com",
    sshPort: 22,
    sshUsername: "deploy",
    identityFilePath: null,
    hasStoredPassword: true,
    localBindAddress: "127.0.0.1",
    localPort: id === "pg" ? 5432 : 6379,
    remoteHost: "10.0.4.12",
    remotePort: id === "pg" ? 5432 : 6379,
    keepAliveIntervalSec: 30,
    keepAliveMaxCount: 5,
    groupId: null,
    tags: [],
  };
}

function input(): HTMLInputElement {
  return screen.getByRole("combobox") as HTMLInputElement;
}

describe("CommandPalette", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    paletteQuery.set("");
    groups.set([]);
    forwards.set([mk("pg", "Postgres"), mk("rd", "Redis")]);
    statusById.set({ pg: "disconnected", rd: "connected" });
  });

  it("lists tunnels and actions", () => {
    render(CommandPalette);
    expect(screen.getByText("Postgres")).toBeInTheDocument();
    expect(screen.getByText("Start all tunnels")).toBeInTheDocument();
  });

  it("fuzzy-filters to the matching tunnel and connects it on Enter", async () => {
    render(CommandPalette);
    await fireEvent.input(input(), { target: { value: "postgres" } });
    expect(screen.queryByText("Redis")).not.toBeInTheDocument();
    await fireEvent.keyDown(input(), { key: "Enter" });
    expect(connectForward).toHaveBeenCalledWith("pg");
    expect(disconnectForward).not.toHaveBeenCalled();
  });

  it("disconnects a connected tunnel (context-aware primary action)", async () => {
    render(CommandPalette);
    await fireEvent.input(input(), { target: { value: "redis" } });
    await fireEvent.keyDown(input(), { key: "Enter" });
    expect(disconnectForward).toHaveBeenCalledWith("rd");
  });

  it("runs the Start all action", async () => {
    render(CommandPalette);
    await fireEvent.input(input(), { target: { value: "start all" } });
    await fireEvent.keyDown(input(), { key: "Enter" });
    expect(startAll).toHaveBeenCalledTimes(1);
  });

  it("shows a no-results state for an unmatched query", async () => {
    render(CommandPalette);
    await fireEvent.input(input(), { target: { value: "zzzznomatch" } });
    expect(screen.getByText(/no results for/i)).toBeInTheDocument();
  });
});
