// @vitest-environment jsdom
/**
 * Web-mode transport (spec 02 §10): HTTP invoke + the shared event socket.
 * `fetch` and `WebSocket` are faked; every test loads a fresh module so the
 * socket/backoff state never leaks between cases.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./runtime", () => ({ isWeb: true }));

type Transport = typeof import("./transport");

class FakeSocket {
  static all: FakeSocket[] = [];
  readyState = 0;
  closedWith: number | null = null;
  onopen: ((e: Event) => void) | null = null;
  onmessage: ((e: MessageEvent) => void) | null = null;
  onclose: ((e: CloseEvent) => void) | null = null;
  onerror: ((e: Event) => void) | null = null;

  constructor(public url: string) {
    FakeSocket.all.push(this);
  }
  close(code?: number): void {
    this.readyState = 3;
    this.closedWith = code ?? 1000;
  }
  // --- test drivers ---
  open(): void {
    this.readyState = 1;
    this.onopen?.(new Event("open"));
  }
  send(frame: unknown): void {
    const data = typeof frame === "string" ? frame : JSON.stringify(frame);
    this.onmessage?.({ data } as MessageEvent);
  }
  drop(code = 1006): void {
    this.readyState = 3;
    this.onclose?.({ code } as CloseEvent);
  }
}

const latest = (): FakeSocket => FakeSocket.all[FakeSocket.all.length - 1];

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

const fetchMock = vi.fn<typeof fetch>();
let t: Transport;

beforeEach(async () => {
  FakeSocket.all = [];
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("WebSocket", FakeSocket);
  vi.resetModules();
  t = await import("./transport");
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("invoke over HTTP", () => {
  it("POSTs the args to /api/invoke/<command> with the CSRF header", async () => {
    fetchMock.mockResolvedValue(json(200, [{ id: "a" }]));

    const out = await t.invoke<{ id: string }[]>("update_forward", {
      id: "a",
      input: { name: "db" },
    });

    expect(out).toEqual([{ id: "a" }]);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/api/invoke/update_forward");
    expect(init?.method).toBe("POST");
    expect(init?.credentials).toBe("same-origin");
    expect(init?.headers).toMatchObject({ "X-Tunnel-Pilot": "1" });
    expect(JSON.parse(String(init?.body))).toEqual({
      id: "a",
      input: { name: "db" },
    });
  });

  it("sends {} when a command takes no args, and maps unit to null", async () => {
    fetchMock.mockResolvedValueOnce(json(200, null));
    fetchMock.mockResolvedValueOnce(new Response("", { status: 200 }));

    await expect(t.invoke("start_all")).resolves.toBeNull();
    await expect(t.invoke("stop_all")).resolves.toBeNull();
    expect(fetchMock.mock.calls[0][1]?.body).toBe("{}");
  });

  it("rejects with the server's AppError, readable as text", async () => {
    fetchMock.mockResolvedValue(
      json(404, { kind: "notFound", message: "no forward abc" }),
    );

    const err: unknown = await t.invoke("connect_forward", { id: "abc" }).then(
      () => null,
      (e: unknown) => e,
    );

    expect(err).toEqual({ kind: "notFound", message: "no forward abc" });
    expect(String(err)).toBe("no forward abc");
    expect(JSON.stringify(err)).toBe(
      '{"kind":"notFound","message":"no forward abc"}',
    );
  });

  it("maps a non-JSON failure to an internal AppError", async () => {
    fetchMock.mockResolvedValue(
      new Response("<html>bad gateway", { status: 502 }),
    );
    await expect(t.invoke("list_forwards")).rejects.toEqual({
      kind: "internal",
      message: "Server error (HTTP 502)",
    });
  });

  it("maps a network failure to an io AppError", async () => {
    fetchMock.mockRejectedValue(new TypeError("Failed to fetch"));
    await expect(t.invoke("list_forwards")).rejects.toMatchObject({
      kind: "io",
    });
  });

  it("signals the auth layer on 401", async () => {
    const onUnauth = vi.fn();
    t.onUnauthorized(onUnauth);
    fetchMock.mockResolvedValue(
      json(401, { kind: "unauthorized", message: "session expired" }),
    );

    await expect(t.invoke("app_hydrate")).rejects.toMatchObject({
      kind: "unauthorized",
    });
    expect(onUnauth).toHaveBeenCalledOnce();
  });

  it("does not signal the auth layer on other errors", async () => {
    const onUnauth = vi.fn();
    t.onUnauthorized(onUnauth);
    fetchMock.mockResolvedValue(
      json(400, { kind: "invalidInput", message: "x" }),
    );
    await expect(t.invoke("create_forward")).rejects.toBeTruthy();
    expect(onUnauth).not.toHaveBeenCalled();
  });
});

describe("listen over the shared WebSocket", () => {
  it("opens ONE socket for every listener and resolves once it is open", async () => {
    let resolved = 0;
    const a = t
      .listen("tunnel://status", () => {})
      .then((u) => {
        resolved += 1;
        return u;
      });
    const b = t
      .listen("log://line", () => {})
      .then((u) => {
        resolved += 1;
        return u;
      });
    await Promise.resolve();

    expect(FakeSocket.all).toHaveLength(1);
    expect(latest().url).toBe(`ws://${location.host}/api/events`);
    expect(resolved).toBe(0);

    latest().open();
    await Promise.all([a, b]);
    expect(resolved).toBe(2);
  });

  it("uses wss on an https page", async () => {
    vi.stubGlobal("location", { protocol: "https:", host: "tp.example.com" });
    void t.listen("log://line", () => {});
    expect(latest().url).toBe("wss://tp.example.com/api/events");
  });

  it("dispatches {event, payload} frames to the matching listeners only", async () => {
    const status = vi.fn();
    const logs = vi.fn();
    const pending = Promise.all([
      t.listen("tunnel://status", status),
      t.listen("log://line", logs),
    ]);
    latest().open();
    await pending;

    latest().send({
      event: "tunnel://status",
      payload: { id: "a", status: "connected", lastError: null },
    });
    latest().send({ event: "log://cleared", payload: null });
    latest().send("not json");
    latest().send({ payload: 1 });

    expect(status).toHaveBeenCalledOnce();
    expect(status.mock.calls[0][0]).toMatchObject({
      event: "tunnel://status",
      payload: { id: "a", status: "connected", lastError: null },
    });
    expect(logs).not.toHaveBeenCalled();
  });

  it("stops delivering after unlisten and closes the socket with the last one", async () => {
    const cb = vi.fn();
    const pending = t.listen("log://line", cb);
    latest().open();
    const unlisten = await pending;

    unlisten();
    latest().send({ event: "log://line", payload: {} });
    expect(cb).not.toHaveBeenCalled();
    expect(latest().closedWith).toBe(1000);
  });

  it("reconnects with backoff and asks for a resync after the gap", async () => {
    vi.useFakeTimers();
    const resumed = vi.fn();
    t.onReconnect(resumed);
    const pending = t.listen("log://line", () => {});
    latest().open();
    await pending;
    expect(resumed).not.toHaveBeenCalled();

    latest().drop(1006);
    expect(FakeSocket.all).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(499);
    expect(FakeSocket.all).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeSocket.all).toHaveLength(2);

    // The retry's handshake fails: probe the session, then back off longer.
    fetchMock.mockResolvedValue(
      json(200, { authenticated: true, version: "2.3.0", edition: "cli" }),
    );
    latest().drop(1006);
    await vi.advanceTimersByTimeAsync(0);
    expect(fetchMock).toHaveBeenCalledWith("/api/session", expect.anything());
    await vi.advanceTimersByTimeAsync(999);
    expect(FakeSocket.all).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(FakeSocket.all).toHaveLength(3);

    latest().open();
    expect(resumed).toHaveBeenCalledOnce();
  });

  it("returns to login when the server ends the session (close 4401)", async () => {
    vi.useFakeTimers();
    const onUnauth = vi.fn();
    t.onUnauthorized(onUnauth);
    const pending = t.listen("log://line", () => {});
    latest().open();
    await pending;

    latest().drop(t.WS_SESSION_ENDED);
    expect(onUnauth).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(FakeSocket.all).toHaveLength(1);
  });

  it("returns to login when a refused handshake turns out to be an expired session", async () => {
    vi.useFakeTimers();
    const onUnauth = vi.fn();
    t.onUnauthorized(onUnauth);
    fetchMock.mockResolvedValue(
      json(200, { authenticated: false, version: "2.3.0", edition: "cli" }),
    );
    const pending = t.listen("log://line", () => {});
    latest().drop(1006);
    await pending;
    await vi.advanceTimersByTimeAsync(0);

    expect(onUnauth).toHaveBeenCalledOnce();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(FakeSocket.all).toHaveLength(1);
  });

  it("does not reconnect once every listener is gone", async () => {
    vi.useFakeTimers();
    const pending = t.listen("log://line", () => {});
    latest().open();
    const unlisten = await pending;
    unlisten();
    latest().drop(1006);
    await vi.advanceTimersByTimeAsync(30_000);
    expect(FakeSocket.all).toHaveLength(1);
  });
});
