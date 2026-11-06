// @vitest-environment jsdom
/** Web-mode helpers: sign-in outcomes, clipboard fallback, HTTP warning. */
import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

vi.mock("./runtime", () => ({ isWeb: true }));

import {
  authPhase,
  sessionEnded,
  signIn,
  signOut,
  watchUnauthorized,
} from "./auth";
import { copyText, isInsecureRemote, isLoopbackHost } from "./platform";

const fetchMock = vi.fn<typeof fetch>();

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
  authPhase.set("checking");
  sessionEnded.set(false);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("auth", () => {
  it("signs in on 204 and sends the CSRF header", async () => {
    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }));
    fetchMock.mockResolvedValue(
      new Response('{"authenticated":true,"version":"2.3.0","edition":"cli"}'),
    );
    await expect(signIn("pw")).resolves.toEqual({ ok: true });
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/api/login");
    expect(init?.headers).toMatchObject({ "X-Tunnel-Pilot": "1" });
    expect(JSON.parse(String(init?.body))).toEqual({ password: "pw" });
    expect(get(authPhase)).toBe("signedIn");
  });

  it("maps 401 and 429 (with Retry-After)", async () => {
    fetchMock.mockResolvedValueOnce(new Response("{}", { status: 401 }));
    await expect(signIn("x")).resolves.toEqual({
      ok: false,
      reason: "invalid",
    });

    fetchMock.mockResolvedValueOnce(
      new Response("", { status: 429, headers: { "Retry-After": "90" } }),
    );
    await expect(signIn("x")).resolves.toEqual({
      ok: false,
      reason: "rateLimited",
      retryAfterSec: 90,
    });
    expect(get(authPhase)).toBe("checking");
  });

  it("treats a lost session as 'ended' only when one was live", () => {
    const stop = watchUnauthorized();
    authPhase.set("signedIn");
    // Fire through the transport's signal by importing it lazily.
    return import("./transport").then(async (t) => {
      fetchMock.mockResolvedValue(new Response("{}", { status: 401 }));
      await t.invoke("list_forwards").catch(() => {});
      expect(get(authPhase)).toBe("signedOut");
      expect(get(sessionEnded)).toBe(true);
      stop();
    });
  });

  it("signs out on 204 and refuses to pretend when the server is down", async () => {
    authPhase.set("signedIn");
    fetchMock.mockRejectedValueOnce(new TypeError("Failed to fetch"));
    await expect(signOut()).rejects.toThrow();
    expect(get(authPhase)).toBe("signedIn");

    fetchMock.mockResolvedValueOnce(new Response(null, { status: 204 }));
    await signOut();
    expect(get(authPhase)).toBe("signedOut");
    expect(get(sessionEnded)).toBe(false);
  });
});

describe("copyText", () => {
  it("falls back to execCommand outside a secure context and keeps focus", async () => {
    const button = document.createElement("button");
    document.body.appendChild(button);
    button.focus();
    vi.stubGlobal("isSecureContext", false);
    const exec = vi.fn(() => true);
    document.execCommand = exec;

    await copyText("ssh -N -L 5432:db:5432 me@bastion");

    expect(exec).toHaveBeenCalledWith("copy");
    expect(document.querySelector("textarea")).toBeNull();
    expect(button).toHaveFocus();
    button.remove();
  });

  it("rejects when the browser blocks the copy", async () => {
    vi.stubGlobal("isSecureContext", false);
    document.execCommand = vi.fn(() => false);
    await expect(copyText("x")).rejects.toThrow(/clipboard/);
  });

  it("uses the async clipboard API in a secure context", async () => {
    vi.stubGlobal("isSecureContext", true);
    const writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    await copyText("hello");
    expect(writeText).toHaveBeenCalledWith("hello");
  });
});

describe("plain-HTTP warning", () => {
  it.each([
    ["localhost", true],
    ["app.localhost", true],
    ["127.0.0.1", true],
    ["127.4.5.6", true],
    ["[::1]", true],
    ["10.0.0.5", false],
    ["tp.example.com", false],
  ])("%s is loopback: %s", (host, loopback) => {
    expect(isLoopbackHost(host)).toBe(loopback);
  });

  it("warns only for http from a non-loopback host", () => {
    expect(isInsecureRemote({ protocol: "http:", hostname: "10.0.0.5" })).toBe(
      true,
    );
    expect(isInsecureRemote({ protocol: "https:", hostname: "10.0.0.5" })).toBe(
      false,
    );
    expect(isInsecureRemote({ protocol: "http:", hostname: "127.0.0.1" })).toBe(
      false,
    );
  });
});
