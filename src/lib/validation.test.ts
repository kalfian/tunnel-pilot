import { describe, expect, it } from "vitest";

import {
  PORT_MAX,
  PORT_MIN,
  isFormValid,
  isValidPort,
  validateForwardForm,
  type ForwardFormValues,
} from "./validation";

function validForm(over: Partial<ForwardFormValues> = {}): ForwardFormValues {
  return {
    name: "web",
    sshHost: "bastion.example.com",
    sshPort: 22,
    sshUsername: "deploy",
    localBindAddress: "127.0.0.1",
    localPort: 8080,
    remoteHost: "localhost",
    remotePort: 80,
    keepAliveIntervalSec: 0,
    keepAliveMaxCount: 0,
    identityFilePath: null,
    password: "",
    ...over,
  };
}

describe("isValidPort", () => {
  it("accepts the boundary ports", () => {
    expect(isValidPort(PORT_MIN)).toBe(true);
    expect(isValidPort(PORT_MAX)).toBe(true);
  });

  it("rejects out-of-range and non-integer ports", () => {
    expect(isValidPort(0)).toBe(false);
    expect(isValidPort(65536)).toBe(false);
    expect(isValidPort(-1)).toBe(false);
    expect(isValidPort(22.5)).toBe(false);
    expect(isValidPort(Number.NaN)).toBe(false);
  });
});

describe("validateForwardForm", () => {
  it("passes a fully valid form", () => {
    const errors = validateForwardForm(validForm());
    expect(errors).toEqual({});
    expect(isFormValid(errors)).toBe(true);
  });

  it("flags missing required text fields", () => {
    const errors = validateForwardForm(
      validForm({ name: "  ", sshHost: "", sshUsername: "", remoteHost: "" }),
    );
    expect(errors.name).toBeDefined();
    expect(errors.sshHost).toBeDefined();
    expect(errors.sshUsername).toBeDefined();
    expect(errors.remoteHost).toBeDefined();
    expect(isFormValid(errors)).toBe(false);
  });

  it("flags every out-of-range port", () => {
    const errors = validateForwardForm(
      validForm({ sshPort: 0, localPort: 70000, remotePort: -5 }),
    );
    expect(errors.sshPort).toBeDefined();
    expect(errors.localPort).toBeDefined();
    expect(errors.remotePort).toBeDefined();
  });

  it("rejects negative or non-integer keep-alive fields", () => {
    const errors = validateForwardForm(
      validForm({ keepAliveIntervalSec: -1, keepAliveMaxCount: 2.5 }),
    );
    expect(errors.keepAliveIntervalSec).toBeDefined();
    expect(errors.keepAliveMaxCount).toBeDefined();
  });

  it("allows zero keep-alive values (use-default sentinel)", () => {
    const errors = validateForwardForm(
      validForm({ keepAliveIntervalSec: 0, keepAliveMaxCount: 0 }),
    );
    expect(errors.keepAliveIntervalSec).toBeUndefined();
    expect(errors.keepAliveMaxCount).toBeUndefined();
  });

  describe("password ⊕ identity exclusivity", () => {
    it("allows password only", () => {
      const errors = validateForwardForm(validForm({ password: "hunter2" }));
      expect(errors.auth).toBeUndefined();
    });

    it("allows identity only", () => {
      const errors = validateForwardForm(
        validForm({ identityFilePath: "/home/u/.ssh/id_ed25519" }),
      );
      expect(errors.auth).toBeUndefined();
    });

    it("allows neither (agent / no auth)", () => {
      const errors = validateForwardForm(validForm());
      expect(errors.auth).toBeUndefined();
    });

    it("rejects a typed password together with an identity file", () => {
      const errors = validateForwardForm(
        validForm({
          password: "hunter2",
          identityFilePath: "/home/u/.ssh/id_ed25519",
        }),
      );
      expect(errors.auth).toBeDefined();
    });

    it("treats an existing stored password as a password on edit", () => {
      const errors = validateForwardForm(
        validForm({
          password: "",
          hasStoredPassword: true,
          identityFilePath: "/home/u/.ssh/id_ed25519",
        }),
      );
      expect(errors.auth).toBeDefined();
    });
  });
});
