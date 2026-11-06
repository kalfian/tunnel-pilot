import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";

import {
  EMPTY_STATS,
  applyForwards,
  applyRuntimes,
  applyStats,
  applyStatus,
  clearHostKeyMismatch,
  connectedCount,
  forwards,
  hostKeyMismatchById,
  lastErrorById,
  statsById,
  statusById,
} from "./forwards";
import type { ForwardConfig, HostKeyMismatch, TunnelStats } from "../types";

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
    hostKey: null,
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
  hostKeyMismatchById.set({});
});

const MISMATCH: HostKeyMismatch = {
  expected: null,
  presented: {
    algorithm: "ssh-ed25519",
    fingerprint: "SHA256:presented",
    pinnedAt: null,
  },
  source: "knownHosts",
};

describe("applyForwards", () => {
  it("seeds neutral defaults for every id", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    expect(get(statusById)).toEqual({ a: "disconnected", b: "disconnected" });
    expect(get(statsById)).toEqual({ a: EMPTY_STATS, b: EMPTY_STATS });
    expect(get(lastErrorById)).toEqual({ a: null, b: null });
  });

  it("preserves live status for surviving ids on re-set (reorder/rename)", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    applyStatus({
      id: "a",
      status: "connected",
      lastError: null,
      hostKeyMismatch: null,
    });

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
    applyStatus({
      id: "b",
      status: "error",
      lastError: "connection refused",
      hostKeyMismatch: null,
    });

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
          hostKeyMismatch: null,
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

    applyStatus({
      id: "a",
      status: "connected",
      lastError: null,
      hostKeyMismatch: null,
    });
    applyStatus({
      id: "b",
      status: "connecting",
      lastError: null,
      hostKeyMismatch: null,
    });
    expect(get(connectedCount)).toBe(1);

    applyStatus({
      id: "b",
      status: "connected",
      lastError: null,
      hostKeyMismatch: null,
    });
    expect(get(connectedCount)).toBe(2);
  });
});

describe("host key mismatch", () => {
  it("is seeded null, set by a status event, and pruned with its forward", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    expect(get(hostKeyMismatchById)).toEqual({ a: null, b: null });

    applyStatus({
      id: "a",
      status: "error",
      lastError: "host key for bastion:22 does not match …",
      hostKeyMismatch: MISMATCH,
    });
    expect(get(hostKeyMismatchById)).toEqual({ a: MISMATCH, b: null });

    applyForwards([makeForward("b")]);
    expect(get(hostKeyMismatchById)).toEqual({ b: null });
  });

  it("tolerates a status payload without the field (older backend)", () => {
    applyForwards([makeForward("a")]);
    applyStatus({
      id: "a",
      status: "error",
      lastError: "boom",
    } as unknown as Parameters<typeof applyStatus>[0]);
    expect(get(hostKeyMismatchById).a).toBeNull();
  });

  it("comes from hydrate runtimes and survives a forwards re-set", () => {
    applyForwards([makeForward("a")]);
    applyRuntimes([
      [
        "a",
        {
          status: "error",
          stats: EMPTY_STATS,
          lastError: "host key changed",
          hostKeyMismatch: MISMATCH,
        },
      ],
    ]);
    applyForwards([makeForward("a", "renamed")]);
    expect(get(hostKeyMismatchById).a).toEqual(MISMATCH);
  });

  it("clearHostKeyMismatch drops only that tunnel's refusal", () => {
    applyForwards([makeForward("a"), makeForward("b")]);
    hostKeyMismatchById.set({ a: MISMATCH, b: MISMATCH });
    clearHostKeyMismatch("a");
    expect(get(hostKeyMismatchById)).toEqual({ a: null, b: MISMATCH });
    // Unknown ids are a no-op, not a new entry.
    clearHostKeyMismatch("zzz");
    expect(get(hostKeyMismatchById)).not.toHaveProperty("zzz");
  });
});
