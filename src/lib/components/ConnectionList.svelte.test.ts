// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/svelte";
import type {
  ForwardConfig,
  ForwardStatus,
  TunnelGroup,
  TunnelStats,
} from "../types";
import ConnectionList from "./ConnectionList.svelte";

vi.mock("../ipc", () => ({
  reorderForwards: vi.fn(() => Promise.resolve()),
  updateGroup: vi.fn(() => Promise.resolve({})),
  assignForwardGroup: vi.fn(() => Promise.resolve()),
  startGroup: vi.fn(() => Promise.resolve()),
  stopGroup: vi.fn(() => Promise.resolve()),
  connectForward: vi.fn(() => Promise.resolve()),
  disconnectForward: vi.fn(() => Promise.resolve()),
  retryForward: vi.fn(() => Promise.resolve()),
  duplicateForward: vi.fn(() => Promise.resolve()),
  copySshCommand: vi.fn(() => Promise.resolve("ssh ...")),
}));
vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({
  writeText: vi.fn(() => Promise.resolve()),
}));

import {
  reorderForwards,
  updateGroup,
  startGroup,
  assignForwardGroup,
} from "../ipc";

function mk(id: string, name: string, groupId: string | null, tags: string[] = []): ForwardConfig {
  return {
    id,
    name,
    sshHost: "bastion.example.com",
    sshPort: 22,
    sshUsername: "deploy",
    identityFilePath: null,
    hasStoredPassword: true,
    localBindAddress: "127.0.0.1",
    localPort: 5432,
    remoteHost: "10.0.4.12",
    remotePort: 5432,
    keepAliveIntervalSec: 30,
    keepAliveMaxCount: 5,
    groupId,
    tags,
  };
}

const GROUPS: TunnelGroup[] = [
  { id: "g1", name: "Production", color: null, order: 0, collapsed: false },
];
// a,b live in Production; c is ungrouped.
const FORWARDS = [
  mk("a", "Alpha", "g1", ["db"]),
  mk("b", "Bravo", "g1"),
  mk("c", "Charlie", null),
];

const EMPTY: TunnelStats = {
  activeConnections: 0,
  totalBytesUp: 0,
  totalBytesDown: 0,
  lastPingLatencyMs: null,
  connectedSince: null,
};

function renderList(
  over: {
    groups?: TunnelGroup[];
    filterQuery?: string;
    activeTag?: string | null;
    status?: Record<string, ForwardStatus>;
  } = {},
) {
  return render(ConnectionList, {
    props: {
      forwards: FORWARDS,
      groups: over.groups ?? GROUPS,
      statusById: over.status ?? {},
      statsById: { a: EMPTY, b: EMPTY, c: EMPTY },
      lastErrorById: {},
      selectedId: null,
      filterQuery: over.filterQuery ?? "",
      activeTag: over.activeTag ?? null,
      onSelect: vi.fn(),
      onEdit: vi.fn(),
      onDelete: vi.fn(),
      onViewLog: vi.fn(),
      onEditGroup: vi.fn(),
      onDeleteGroup: vi.fn(),
    },
  });
}

function rowBody(id: string): HTMLElement {
  return document.querySelector(`[data-row-id="${id}"]`) as HTMLElement;
}

describe("ConnectionList — groups", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders a group header + an ungrouped section", () => {
    renderList();
    expect(screen.getByText("Production")).toBeInTheDocument();
    expect(screen.getByText("Ungrouped")).toBeInTheDocument();
  });

  it("shows a flat list (no headers) when there are no groups", () => {
    renderList({ groups: [] });
    expect(screen.queryByText("Ungrouped")).not.toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
  });

  it("persists a collapse toggle via update_group", async () => {
    renderList();
    // The disclosure (not the ⋯ actions button) is the one with aria-expanded.
    const header = screen.getByRole("button", {
      name: /production/i,
      expanded: true,
    });
    await fireEvent.click(header);
    expect(updateGroup).toHaveBeenCalledWith("g1", {
      name: "Production",
      color: null,
      collapsed: true,
    });
  });

  it("starts a group from its header", async () => {
    renderList();
    await fireEvent.click(screen.getByRole("button", { name: /start all/i }));
    expect(startGroup).toHaveBeenCalledWith("g1");
  });

  it("filters by tag to just the tagged tunnel", () => {
    renderList({ activeTag: "db" });
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Bravo")).not.toBeInTheDocument();
    expect(screen.queryByText("Charlie")).not.toBeInTheDocument();
  });
});

describe("ConnectionList — group management (Feature A)", () => {
  beforeEach(() => vi.clearAllMocks());

  function renderWithSpies() {
    const onEditGroup = vi.fn();
    const onDeleteGroup = vi.fn();
    render(ConnectionList, {
      props: {
        forwards: FORWARDS,
        groups: GROUPS,
        statusById: {},
        statsById: { a: EMPTY, b: EMPTY, c: EMPTY },
        lastErrorById: {},
        selectedId: null,
        filterQuery: "",
        activeTag: null,
        onSelect: vi.fn(),
        onEdit: vi.fn(),
        onDelete: vi.fn(),
        onViewLog: vi.fn(),
        onEditGroup,
        onDeleteGroup,
      },
    });
    return { onEditGroup, onDeleteGroup };
  }

  it("requests editing a group from its ⋯ menu", async () => {
    const { onEditGroup } = renderWithSpies();
    await fireEvent.click(
      screen.getByRole("button", { name: /production group actions/i }),
    );
    await fireEvent.click(screen.getByRole("menuitem", { name: /edit group/i }));
    expect(onEditGroup).toHaveBeenCalledWith(GROUPS[0]);
  });

  it("requests deleting a group from its ⋯ menu", async () => {
    const { onDeleteGroup } = renderWithSpies();
    await fireEvent.click(
      screen.getByRole("button", { name: /production group actions/i }),
    );
    await fireEvent.click(
      screen.getByRole("menuitem", { name: /delete group/i }),
    );
    expect(onDeleteGroup).toHaveBeenCalledWith(GROUPS[0]);
  });

  it("assigns a tunnel to a group from the row ⋯ menu", async () => {
    renderWithSpies();
    // Charlie is ungrouped → move it into Production via the row menu submenu.
    const charlieRow = rowBody("c").closest(".card") as HTMLElement;
    await fireEvent.click(
      within(charlieRow).getByRole("button", { name: /tunnel actions/i }),
    );
    await fireEvent.click(
      screen.getByRole("menuitem", { name: /assign group/i }),
    );
    // Submenu now lists groups; pick Production.
    const items = screen.getAllByRole("menuitem", { name: "Production" });
    await fireEvent.click(items[items.length - 1]);
    expect(assignForwardGroup).toHaveBeenCalledWith("c", "g1");
  });
});

describe("ConnectionList — drag tunnels between groups (Feature #1)", () => {
  beforeEach(() => vi.clearAllMocks());

  function sectionEl(id: string): HTMLElement {
    return document.querySelector(`[data-section="${id}"]`) as HTMLElement;
  }
  function liOf(id: string): HTMLElement {
    return rowBody(id).closest("li") as HTMLElement;
  }

  it("dropping a tunnel on a different group's section reassigns it", async () => {
    renderList();
    // Charlie (ungrouped) dragged onto the Production (g1) section.
    await fireEvent.dragStart(liOf("c"));
    const target = sectionEl("g1");
    await fireEvent.dragOver(target);
    await fireEvent.drop(target);
    expect(assignForwardGroup).toHaveBeenCalledWith("c", "g1");
    // A cross-group move must not also fire a reorder.
    expect(reorderForwards).not.toHaveBeenCalled();
  });

  it("dropping a tunnel on the Ungrouped section clears its group (null)", async () => {
    renderList();
    // Alpha (in g1) dragged onto the Ungrouped section.
    await fireEvent.dragStart(liOf("a"));
    const target = sectionEl("__ungrouped__");
    await fireEvent.dragOver(target);
    await fireEvent.drop(target);
    expect(assignForwardGroup).toHaveBeenCalledWith("a", null);
    expect(reorderForwards).not.toHaveBeenCalled();
  });

  it("dropping on a different group's row (not just header) reassigns", async () => {
    renderList();
    // Charlie dropped onto Alpha's row — Alpha lives in g1, so this reassigns.
    await fireEvent.dragStart(liOf("c"));
    await fireEvent.dragOver(liOf("a"));
    await fireEvent.drop(liOf("a"));
    expect(assignForwardGroup).toHaveBeenCalledWith("c", "g1");
    expect(reorderForwards).not.toHaveBeenCalled();
  });

  it("dragging within the same group still reorders (no reassign)", async () => {
    renderList();
    // Alpha over Bravo — both in g1 → reorder, not reassign.
    await fireEvent.dragStart(liOf("a"));
    await fireEvent.dragOver(liOf("b"));
    await fireEvent.dragEnd(liOf("a"));
    expect(reorderForwards).toHaveBeenCalledTimes(1);
    const arg = vi.mocked(reorderForwards).mock.calls[0][0];
    expect([...arg].sort()).toEqual(["a", "b", "c"]);
    expect(arg).toEqual(["b", "a", "c"]);
    expect(assignForwardGroup).not.toHaveBeenCalled();
  });

  it("a no-move drag persists nothing", async () => {
    renderList();
    await fireEvent.dragStart(liOf("a"));
    await fireEvent.dragEnd(liOf("a"));
    expect(reorderForwards).not.toHaveBeenCalled();
    expect(assignForwardGroup).not.toHaveBeenCalled();
  });
});

describe("ConnectionList — F43 reorder safety", () => {
  beforeEach(() => vi.clearAllMocks());

  it("keyboard reorder persists the FULL ordered id list", async () => {
    renderList();
    rowBody("a").focus();
    await fireEvent.keyDown(rowBody("a"), { key: "ArrowDown", altKey: true });
    expect(reorderForwards).toHaveBeenCalledTimes(1);
    const arg = vi.mocked(reorderForwards).mock.calls[0][0];
    // Every id is present (never a filtered subset) and Alpha/Bravo swapped.
    expect([...arg].sort()).toEqual(["a", "b", "c"]);
    expect(arg).toEqual(["b", "a", "c"]);
  });

  it("disables drag + keyboard reorder while a filter is active", async () => {
    renderList({ filterQuery: "alpha" });
    const li = rowBody("a").closest("li") as HTMLElement;
    // Under a filter the row must not be draggable (F43).
    expect(li.getAttribute("draggable")).toBe("false");
    rowBody("a").focus();
    await fireEvent.keyDown(rowBody("a"), { key: "ArrowDown", altKey: true });
    expect(reorderForwards).not.toHaveBeenCalled();
  });
});
