// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { reloadIfStale } from "./auth";
import { APP_VERSION } from "./version";

describe("reloadIfStale", () => {
  beforeEach(() => sessionStorage.clear());

  it("does nothing when the server runs this bundle's version", () => {
    const reload = vi.fn();
    expect(reloadIfStale(APP_VERSION, reload)).toBe(false);
    expect(reloadIfStale(undefined, reload)).toBe(false);
    expect(reload).not.toHaveBeenCalled();
  });

  it("reloads once per newer server version, never in a loop", () => {
    const reload = vi.fn();
    expect(reloadIfStale("99.0.0", reload)).toBe(true);
    expect(reloadIfStale("99.0.0", reload)).toBe(false);
    expect(reload).toHaveBeenCalledTimes(1);
    expect(reloadIfStale("99.0.1", reload)).toBe(true);
    expect(reload).toHaveBeenCalledTimes(2);
  });
});
