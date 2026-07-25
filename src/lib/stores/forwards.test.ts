import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";

import {
  EMPTY_STATS,
  applyForwards,
  applyRuntimes,
  applyStats,
  applyStatus,
  connectedCount,
  forwards,
  lastErrorById,
  statsById,
  statusById,
} from "./forwards";
import type { ForwardConfig, TunnelStats } from "../types";

function makeForward(id: string, name = id): ForwardConfig {
  return {
    id,
    name,
    sshHost: "example.com",
    sshPort: 22,
    sshUsername: "user",
    identityFilePath: null,
    hasStoredPassword: false,
    localBindAddress: "127.0.0.1",
    localPort: 8080,
    remoteHost: "localhost",
    remotePort: 80,
    keepAliveIntervalSec: 0,
    keepAliveMaxCount: 0,
    groupId: null,
    tags: [],
  };
}

const statsWith = (up: number): TunnelStats => ({
  ...EMPTY_STATS,
  totalBytesUp: up,
});

beforeEach(() => {
  forwards.set([]);
  statusById.set({});
  statsById.set({});
  lastErrorById.set({});
});

describe("applyForwards", () => {
  it("seeds neutral defaults for every id", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    expect(get(statusById)).toEqual({ a: "disconnected", b: "disconnected" });
    expect(get(statsById)).toEqual({ a: EMPTY_STATS, b: EMPTY_STATS });
    expect(get(lastErrorById)).toEqual({ a: null, b: null });
  });

  it("preserves live status for surviving ids on re-set (reorder/rename)", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    applyStatus({ id: "a", status: "connected", lastError: null });

    // Reorder: same ids, different order — must not clobber a's status.
    applyForwards([makeForward("b"), makeForward("a")]);
    expect(get(statusById).a).toBe("connected");
    expect(get(statusById).b).toBe("disconnected");
  });

  it("prunes entries for removed ids", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    applyStats({ id: "a", stats: statsWith(10) });

    applyForwards([makeForward("a")]);
    expect(Object.keys(get(statusById))).toEqual(["a"]);
    expect(Object.keys(get(statsById))).toEqual(["a"]);
    expect(get(statsById).a).toEqual(statsWith(10));
  });
});

describe("applyStatus", () => {
  it("updates only the targeted tunnel and its error", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    applyStatus({ id: "b", status: "error", lastError: "connection refused" });

    expect(get(statusById)).toEqual({ a: "disconnected", b: "error" });
    expect(get(lastErrorById).b).toBe("connection refused");
    expect(get(lastErrorById).a).toBeNull();
  });
});

describe("applyStats", () => {
  it("updates only the targeted tunnel's stats", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    applyStats({ id: "a", stats: statsWith(2048) });

    expect(get(statsById).a).toEqual(statsWith(2048));
    expect(get(statsById).b).toEqual(EMPTY_STATS);
  });
});

describe("applyRuntimes", () => {
  it("seeds status/stats/error from a snapshot", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    applyRuntimes([
      [
        "a",
        {
          status: "connected",
          stats: statsWith(99),
          lastError: null,
        },
      ],
    ]);
    expect(get(statusById).a).toBe("connected");
    expect(get(statsById).a).toEqual(statsWith(99));
    // b had no runtime → keeps seeded defaults.
    expect(get(statusById).b).toBe("disconnected");
  });
});

describe("connectedCount", () => {
  it("counts tunnels in the connected state", () => {
    applyForwards([makeForward("a"), makeForward("b"), makeForward("c")]);
    expect(get(connectedCount)).toBe(0);

    applyStatus({ id: "a", status: "connected", lastError: null });
    applyStatus({ id: "b", status: "connecting", lastError: null });
    expect(get(connectedCount)).toBe(1);

    applyStatus({ id: "b", status: "connected", lastError: null });
    expect(get(connectedCount)).toBe(2);
  });
});
