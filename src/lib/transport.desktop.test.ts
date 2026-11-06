import { describe, expect, it, vi } from "vitest";

vi.mock("./runtime", () => ({ isWeb: false }));
const tauriInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: tauriInvoke }));

import { invoke } from "./transport";

describe("invoke over Tauri (desktop)", () => {
  it("passes results through", async () => {
    tauriInvoke.mockResolvedValueOnce([1, 2]);
    await expect(invoke("list_forwards")).resolves.toEqual([1, 2]);
    expect(tauriInvoke).toHaveBeenCalledWith("list_forwards", undefined);
  });

  it("turns a serialized AppError into one that reads as its message", async () => {
    tauriInvoke.mockRejectedValueOnce({ kind: "ssh", message: "auth failed" });
    const err = await invoke("connect_forward", { id: "x" }).catch(
      (e: unknown) => e,
    );
    expect(err).toEqual({ kind: "ssh", message: "auth failed" });
    expect(String(err)).toBe("auth failed");
  });

  it("leaves unknown rejection shapes untouched", async () => {
    tauriInvoke.mockRejectedValueOnce("plain string");
    await expect(invoke("get_logs")).rejects.toBe("plain string");
  });
});
