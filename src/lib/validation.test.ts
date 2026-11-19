import { describe, expect, it } from "vitest";

import {
  PORT_MAX,
  PORT_MIN,
  checkUsernameVariables,
  isFormValid,
  isValidPort,
  parseTemplate,
  referencedVariables,
  resolveUsername,
  validateForwardForm,
  validateVariableForm,
  variableUses,
  type ForwardFormValues,
} from "./validation";
import type { ForwardConfig, Variable } from "./types";

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
    // Default to a valid single auth method so the base form is valid; auth
    // tests override this explicitly.
    password: "hunter2",
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
        validForm({
          password: "",
          identityFilePath: "/home/u/.ssh/id_ed25519",
        }),
      );
      expect(errors.auth).toBeUndefined();
    });

    it("requires at least one auth method (F46: neither is invalid)", () => {
      const errors = validateForwardForm(
        validForm({ password: "", identityFilePath: null }),
      );
      expect(errors.auth).toBeDefined();
      expect(isFormValid(errors)).toBe(false);
    });

    it("accepts a blank identity path as no identity (still needs auth)", () => {
      const errors = validateForwardForm(
        validForm({ password: "", identityFilePath: "   " }),
      );
      expect(errors.auth).toBeDefined();
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

// --- Variables (spec 03 §24) ---

const VARS: Variable[] = [
  {
    name: "DB_USER",
    secret: false,
    value: "kalfian",
    hasStoredValue: false,
    allowedEndpoints: [],
  },
  {
    name: "ENV",
    secret: false,
    value: "prod",
    hasStoredValue: false,
    allowedEndpoints: [],
  },
  {
    name: "DB_PASS",
    secret: true,
    value: "",
    hasStoredValue: true,
    allowedEndpoints: [],
  },
  {
    name: "NEW_PASS",
    secret: true,
    value: "",
    hasStoredValue: false,
    allowedEndpoints: [],
  },
];

function fwd(over: Partial<ForwardConfig>): ForwardConfig {
  return {
    id: "f1",
    name: "Postgres",
    sshHost: "h",
    sshPort: 22,
    sshUsername: "deploy",
    identityFilePath: null,
    hasStoredPassword: false,
    localBindAddress: "127.0.0.1",
    localPort: 5432,
    remoteHost: "db",
    remotePort: 5432,
    keepAliveIntervalSec: 30,
    keepAliveMaxCount: 5,
    groupId: null,
    tags: [],
    hostKey: null,
    passwordVariable: null,
    ...over,
  };
}

describe("username templates", () => {
  it("parses whole, spaced and embedded references like the backend", () => {
    const parsed = parseTemplate("{{ DB_USER }}-{{ENV}}");
    expect(parsed).toEqual({
      ok: true,
      segments: [
        { kind: "var", name: "DB_USER" },
        { kind: "text", text: "-" },
        { kind: "var", name: "ENV" },
      ],
    });
    expect(referencedVariables("{{A}}{{ A }}x")).toEqual(["A"]);
    expect(referencedVariables("plain")).toEqual([]);
  });

  it("flags an unclosed {{ and an invalid or empty name", () => {
    expect(checkUsernameVariables("{{DB_USER")).toMatch(/unclosed/i);
    expect(checkUsernameVariables("{{1ABC}}")).toMatch(/isn't a valid/i);
    expect(checkUsernameVariables("{{ }}")).toMatch(/empty/i);
  });

  it("flags unknown names, suggesting an exact-case near miss", () => {
    expect(checkUsernameVariables("{{NOPE}}", VARS)).toMatch(
      /isn't a variable.*Settings/,
    );
    expect(checkUsernameVariables("{{db_user}}", VARS)).toMatch(
      /did you mean \{\{DB_USER\}\}/,
    );
  });

  it("refuses a secret variable in a username", () => {
    expect(checkUsernameVariables("{{DB_PASS}}", VARS)).toMatch(/secret/i);
  });

  it("accepts known plain references; syntax-only without a list", () => {
    expect(checkUsernameVariables("{{DB_USER}}-admin", VARS)).toBeNull();
    expect(checkUsernameVariables("{{ANYTHING}}")).toBeNull();
    expect(checkUsernameVariables("deploy", VARS)).toBeNull();
  });

  it("surfaces username variable errors through validateForwardForm", () => {
    const errors = validateForwardForm(
      validForm({ sshUsername: "{{NOPE}}", variables: VARS }),
    );
    expect(errors.sshUsername).toMatch(/isn't a variable/);
  });

  it("resolves the login preview only when every reference is plain", () => {
    expect(resolveUsername("{{DB_USER}}-admin", VARS)).toBe("kalfian-admin");
    expect(resolveUsername("{{ DB_USER }}@{{ENV}}", VARS)).toBe("kalfian@prod");
    expect(resolveUsername("deploy", VARS)).toBeNull();
    expect(resolveUsername("{{NOPE}}", VARS)).toBeNull();
    expect(resolveUsername("{{DB_PASS}}", VARS)).toBeNull();
    expect(resolveUsername("{{DB_USER", VARS)).toBeNull();
  });
});

describe("password variable auth", () => {
  const varAuth = (name: string): Partial<ForwardFormValues> => ({
    password: "",
    identityFilePath: null,
    passwordVariable: name,
    variables: VARS,
  });

  it("accepts a secret variable as the only credential", () => {
    expect(validateForwardForm(validForm(varAuth("DB_PASS"))).auth).toBe(
      undefined,
    );
  });

  it("requires a choice in variable mode", () => {
    expect(validateForwardForm(validForm(varAuth(""))).auth).toMatch(/choose/i);
  });

  it("refuses a plain or missing variable", () => {
    expect(validateForwardForm(validForm(varAuth("DB_USER"))).auth).toMatch(
      /isn't secret/,
    );
    expect(validateForwardForm(validForm(varAuth("GONE"))).auth).toMatch(
      /no longer exists/,
    );
  });

  it("refuses a variable together with an identity file", () => {
    const errors = validateForwardForm(
      validForm({ ...varAuth("DB_PASS"), identityFilePath: "~/.ssh/id" }),
    );
    expect(errors.auth).toMatch(/one way/);
  });
});

describe("variableUses", () => {
  it("counts username references and password use per forward", () => {
    const list = [
      fwd({ id: "a", name: "A", sshUsername: "{{ DB_USER }}-admin" }),
      fwd({ id: "b", name: "B", passwordVariable: "DB_PASS" }),
      fwd({ id: "c", name: "C", sshUsername: "deploy" }),
      fwd({ id: "d", name: "D", sshUsername: "{{DB_USERX}}" }),
    ];
    expect(variableUses(list, "DB_USER")).toEqual([
      { forwardId: "a", forwardName: "A", inUsername: true, asPassword: false },
    ]);
    expect(variableUses(list, "DB_PASS")).toEqual([
      { forwardId: "b", forwardName: "B", inUsername: false, asPassword: true },
    ]);
    expect(variableUses(list, "ENV")).toEqual([]);
  });
});

describe("validateVariableForm", () => {
  const ctx = { variables: VARS, forwards: [] as ForwardConfig[] };

  it("requires a valid, case-insensitively unique name when adding", () => {
    expect(
      validateVariableForm({ name: "", secret: false, value: "x" }, ctx).name,
    ).toBeDefined();
    expect(
      validateVariableForm({ name: "9X", secret: false, value: "x" }, ctx).name,
    ).toMatch(/letters/);
    expect(
      validateVariableForm({ name: "db_user", secret: false, value: "x" }, ctx)
        .name,
    ).toMatch(/DB_USER already exists/);
    expect(
      validateVariableForm({ name: "API", secret: false, value: "x" }, ctx),
    ).toEqual({});
  });

  it("checks plain values like the backend", () => {
    const v = (value: string) =>
      validateVariableForm({ name: "X", secret: false, value }, ctx).value;
    expect(v("")).toBeDefined();
    expect(v("a{{B}}")).toMatch(/nest/);
    expect(v("a\tb")).toMatch(/control/);
    expect(v("x".repeat(257))).toMatch(/256/);
    expect(v("admin\u202Enimda")).toMatch(/invisible/);
    expect(v("-oProxyCommand=sh")).toMatch(/start with/);
    expect(v("ops-team")).toBeUndefined();
  });

  it("lets a blank secret keep a stored value, but not create one", () => {
    const stored = VARS[2];
    const unset = VARS[3];
    expect(
      validateVariableForm(
        { name: stored.name, secret: true, value: "" },
        { ...ctx, existing: stored },
      ),
    ).toEqual({});
    expect(
      validateVariableForm(
        { name: unset.name, secret: true, value: "" },
        { ...ctx, existing: unset },
      ).value,
    ).toBeDefined();
    expect(
      validateVariableForm({ name: "NEW", secret: true, value: "" }, ctx).value,
    ).toBeDefined();
  });

  it("blocks a kind change the forwards using it can't follow", () => {
    const forwards = [
      fwd({ id: "a", name: "Web", sshUsername: "{{DB_USER}}" }),
      fwd({ id: "b", name: "DB", passwordVariable: "DB_PASS" }),
    ];
    expect(
      validateVariableForm(
        { name: "DB_USER", secret: true, value: "s" },
        { variables: VARS, forwards, existing: VARS[0] },
      ).secret,
    ).toMatch(/username of Web/);
    expect(
      validateVariableForm(
        { name: "DB_PASS", secret: false, value: "p" },
        { variables: VARS, forwards, existing: VARS[2] },
      ).secret,
    ).toMatch(/password of DB/);
  });
});
