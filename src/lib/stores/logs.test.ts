import { beforeEach, describe, expect, it } from "vitest";
import { get } from "svelte/store";

import { LOG_CAP, appendLogLine, logs, resetLogs, setLogs } from "./logs";
import type { LogEntry } from "../types";

function makeEntry(message: string): LogEntry {
  return { level: "info", tunnelName: null, message, timestamp: "00:00:00" };
}

beforeEach(() => {
  logs.set([]);
});

describe("appendLogLine", () => {
  it("prepends newest-first", () => {
    appendLogLine(makeEntry("first"));
    appendLogLine(makeEntry("second"));
    expect(get(logs).map((e) => e.message)).toEqual(["second", "first"]);
  });

  it("caps the buffer at LOG_CAP, dropping the oldest", () => {
    for (let i = 0; i < LOG_CAP + 50; i++) {
      appendLogLine(makeEntry(`line-${i}`));
    }
    const buffer = get(logs);
    expect(buffer).toHaveLength(LOG_CAP);
    // Newest is at the front, oldest retained is line-50 (0..49 dropped).
    expect(buffer[0].message).toBe(`line-${LOG_CAP + 49}`);
    expect(buffer[buffer.length - 1].message).toBe("line-50");
  });
});

describe("setLogs", () => {
  it("replaces the buffer and caps defensively", () => {
    const many = Array.from({ length: LOG_CAP + 10 }, (_, i) =>
      makeEntry(`e-${i}`),
    );
    setLogs(many);
    expect(get(logs)).toHaveLength(LOG_CAP);
    expect(get(logs)[0].message).toBe("e-0");
  });
});

describe("resetLogs", () => {
  it("empties the buffer on log://cleared", () => {
    appendLogLine(makeEntry("x"));
    resetLogs();
    expect(get(logs)).toEqual([]);
  });
});
