/**
 * Pure, testable validation rules for the ForwardForm (spec 02 §6.1, 04 §1).
 *
 * The form dialog itself is owned by the ui-ux agent; this module only encodes
 * the rules so they can be unit-tested and shared:
 *   - required text fields,
 *   - port range 1–65535 (SSH port + local/remote ports),
 *   - non-negative keep-alive fields (0 = "use default", spec 03),
 *   - password ⊕ identity-file mutual exclusivity.
 *
 * The password is transient form state (never part of `ForwardInput`; it flows
 * only through `set_forward_password`, spec 02 §6.1). On edit, a forward may
 * already have a stored password — pass `hasStoredPassword` so the exclusivity
 * check accounts for it.
 *
 * Variables (spec 03 §24): a username may reference plain variables as
 * `{{NAME}}`, and a forward may take its password from a secret variable
 * (`passwordVariable`). The template rules below mirror the backend parser
 * (`src-tauri/src/variables.rs`) so a typo is caught while typing, not at
 * save or dial time. Lookups are exact-case, like the backend's.
 */

import type { ForwardConfig, Variable } from "./types";

/** Fields the form can surface an error against. */
export type ForwardFormField =
  | "name"
  | "sshHost"
  | "sshPort"
  | "sshUsername"
  | "localBindAddress"
  | "localPort"
  | "remoteHost"
  | "remotePort"
  | "keepAliveIntervalSec"
  | "keepAliveMaxCount"
  | "auth";

/** Raw values a ForwardForm holds while editing. */
export interface ForwardFormValues {
  name: string;
  sshHost: string;
  sshPort: number;
  sshUsername: string;
  localBindAddress: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  keepAliveIntervalSec: number;
  keepAliveMaxCount: number;
  identityFilePath: string | null;
  /** Password typed into the form this session ("" = none typed). */
  password: string;
  /** True when editing a forward that already has a keychain-stored password. */
  hasStoredPassword?: boolean;
  /** Secret variable used as the password (variable auth mode); null = none. */
  passwordVariable?: string | null;
  /**
   * Known variables. When given, username references and the password
   * variable are checked against them; omitted = template syntax only.
   */
  variables?: readonly Variable[];
}

/** Field → error message. Empty object === valid. */
export type FieldErrors = Partial<Record<ForwardFormField, string>>;

export const PORT_MIN = 1;
export const PORT_MAX = 65535;

/** A valid TCP port: integer in [1, 65535]. */
export function isValidPort(port: number): boolean {
  return Number.isInteger(port) && port >= PORT_MIN && port <= PORT_MAX;
}

function isBlank(value: string): boolean {
  return value.trim().length === 0;
}

/**
 * Validate a ForwardForm. Returns a map of field → message; an empty map means
 * the form is valid. Use {@link isFormValid} for a boolean.
 */
export function validateForwardForm(values: ForwardFormValues): FieldErrors {
  const errors: FieldErrors = {};

  if (isBlank(values.name)) {
    errors.name = "Name is required.";
  }
  if (isBlank(values.sshHost)) {
    errors.sshHost = "SSH host is required.";
  }
  if (isBlank(values.sshUsername)) {
    errors.sshUsername = "SSH username is required.";
  } else {
    const usernameError = checkUsernameVariables(
      values.sshUsername,
      values.variables,
    );
    if (usernameError) errors.sshUsername = usernameError;
  }
  if (isBlank(values.localBindAddress)) {
    errors.localBindAddress = "Local bind address is required.";
  }
  if (isBlank(values.remoteHost)) {
    errors.remoteHost = "Remote host is required.";
  }

  if (!isValidPort(values.sshPort)) {
    errors.sshPort = `SSH port must be between ${PORT_MIN} and ${PORT_MAX}.`;
  }
  if (!isValidPort(values.localPort)) {
    errors.localPort = `Local port must be between ${PORT_MIN} and ${PORT_MAX}.`;
  }
  if (!isValidPort(values.remotePort)) {
    errors.remotePort = `Remote port must be between ${PORT_MIN} and ${PORT_MAX}.`;
  }

  if (
    !Number.isInteger(values.keepAliveIntervalSec) ||
    values.keepAliveIntervalSec < 0
  ) {
    errors.keepAliveIntervalSec =
      "Keep-alive interval must be zero or a positive whole number.";
  }
  if (
    !Number.isInteger(values.keepAliveMaxCount) ||
    values.keepAliveMaxCount < 0
  ) {
    errors.keepAliveMaxCount =
      "Keep-alive max count must be zero or a positive whole number.";
  }

  const hasPassword =
    values.password.trim().length > 0 || values.hasStoredPassword === true;
  const hasIdentity =
    values.identityFilePath !== null && !isBlank(values.identityFilePath);
  const passwordVariable = values.passwordVariable?.trim() ?? "";
  if (values.passwordVariable != null) {
    // Variable auth mode: the variable is the only credential.
    if (passwordVariable === "") {
      errors.auth = "Choose the secret variable that holds the password.";
    } else if (hasPassword || hasIdentity) {
      errors.auth =
        "Use one way to authenticate: a password, an identity file, or a variable.";
    } else if (values.variables) {
      const v = values.variables.find((x) => x.name === passwordVariable);
      if (!v) {
        errors.auth = `${passwordVariable} no longer exists. Choose another variable.`;
      } else if (!v.secret) {
        errors.auth = `${passwordVariable} isn't secret, so it can't be a password.`;
      }
    }
  } else if (hasPassword && hasIdentity) {
    errors.auth = "Use either a password or an identity file, not both.";
  } else if (!hasPassword && !hasIdentity) {
    // At least one auth method is required (v1 intent — a forward with neither
    // can never authenticate). F46: the old rule only enforced exclusivity, so
    // an empty-auth config could be saved and would always fail to connect.
    errors.auth = "Add a password or an identity file to authenticate.";
  }

  return errors;
}

/** True when the given errors map is empty. */
export function isFormValid(errors: FieldErrors): boolean {
  return Object.keys(errors).length === 0;
}

// --- Variables (spec 03 §24) ---

/** A variable name: like an environment variable, at most 64 characters. */
export const VARIABLE_NAME_RE = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/;

/** Limits shared with the backend (`variables.rs`). */
export const PLAIN_VALUE_MAX = 256;
export const SECRET_MAX = 4096;

/** `{{NAME}}` — how a variable is written inside a username. */
export const variableToken = (name: string): string => `{{${name}}}`;

/**
 * The key a secret variable's server binding uses — the same as the backend's
 * `endpoint_key`: host trimmed and ASCII-lowercased, then `:port`.
 */
export const endpointKey = (host: string, port: number): string =>
  `${host.trim().replace(/[A-Z]/g, (c) => c.toLowerCase())}:${port}`;

/** One piece of a username template. */
export type TemplateSegment =
  { kind: "text"; text: string } | { kind: "var"; name: string };

/** A parsed template, or the reason it can't be parsed. */
export type ParsedTemplate =
  { ok: true; segments: TemplateSegment[] } | { ok: false; error: string };

/**
 * Split a username into literal text and `{{NAME}}` references, exactly as
 * the backend does: spaces inside the braces are allowed, an unclosed `{{` or
 * an invalid name is an error.
 */
export function parseTemplate(template: string): ParsedTemplate {
  const segments: TemplateSegment[] = [];
  let rest = template;
  for (;;) {
    const start = rest.indexOf("{{");
    if (start < 0) break;
    if (start > 0) segments.push({ kind: "text", text: rest.slice(0, start) });
    const after = rest.slice(start + 2);
    const end = after.indexOf("}}");
    if (end < 0) {
      return {
        ok: false,
        error: "Unclosed {{ — write a variable as {{NAME}}.",
      };
    }
    const name = after.slice(0, end).trim();
    if (!VARIABLE_NAME_RE.test(name)) {
      return {
        ok: false,
        error:
          name === ""
            ? "Empty {{ }} — put a variable name inside the braces."
            : `${variableToken(name)} isn't a valid variable name.`,
      };
    }
    segments.push({ kind: "var", name });
    rest = after.slice(end + 2);
  }
  if (rest !== "") segments.push({ kind: "text", text: rest });
  return { ok: true, segments };
}

/** Every variable name a template references (empty when unparsable). */
export function referencedVariables(template: string): string[] {
  const parsed = parseTemplate(template);
  if (!parsed.ok) return [];
  const names = parsed.segments.flatMap((s) =>
    s.kind === "var" ? [s.name] : [],
  );
  return [...new Set(names)];
}

/**
 * Why a username's variable references are wrong, or null when they are fine.
 * Without a `variables` list only the template syntax is checked.
 */
export function checkUsernameVariables(
  username: string,
  variables?: readonly Variable[],
): string | null {
  const parsed = parseTemplate(username);
  if (!parsed.ok) return parsed.error;
  if (!variables) return null;
  for (const seg of parsed.segments) {
    if (seg.kind !== "var") continue;
    const v = variables.find((x) => x.name === seg.name);
    if (!v) {
      // Lookups are exact-case; a near miss is almost always a typo.
      const near = variables.find(
        (x) => x.name.toLowerCase() === seg.name.toLowerCase(),
      );
      return near
        ? `${variableToken(seg.name)} isn't a variable — did you mean ${variableToken(near.name)}?`
        : `${variableToken(seg.name)} isn't a variable. Create it in Settings → Variables.`;
    }
    if (v.secret) {
      return `${seg.name} is secret — a username can only use plain variables.`;
    }
  }
  return null;
}

/**
 * The username a template logs in as with today's plain values, or null when
 * it references nothing or can't be resolved (the field error says why).
 */
export function resolveUsername(
  template: string,
  variables: readonly Variable[],
): string | null {
  const parsed = parseTemplate(template.trim());
  if (!parsed.ok || !parsed.segments.some((s) => s.kind === "var")) {
    return null;
  }
  let out = "";
  for (const seg of parsed.segments) {
    if (seg.kind === "text") {
      out += seg.text;
      continue;
    }
    const v = variables.find((x) => x.name === seg.name);
    if (!v || v.secret) return null;
    out += v.value;
  }
  return out;
}

/** Whether a forward uses variable `name` (username reference or password). */
export function forwardUsesVariable(
  forward: Pick<ForwardConfig, "sshUsername" | "passwordVariable">,
  name: string,
): boolean {
  return (
    forward.passwordVariable === name ||
    referencedVariables(forward.sshUsername).includes(name)
  );
}

/** How a forward uses a variable: in its username, as its password, or both. */
export interface VariableUse {
  forwardId: string;
  forwardName: string;
  inUsername: boolean;
  asPassword: boolean;
}

/** Every forward that uses variable `name`, in list order. */
export function variableUses(
  forwards: readonly ForwardConfig[],
  name: string,
): VariableUse[] {
  return forwards.flatMap((f) => {
    const inUsername = referencedVariables(f.sshUsername).includes(name);
    const asPassword = f.passwordVariable === name;
    return inUsername || asPassword
      ? [{ forwardId: f.id, forwardName: f.name, inUsername, asPassword }]
      : [];
  });
}

/** Raw values the variable dialog holds while editing. */
export interface VariableFormValues {
  name: string;
  secret: boolean;
  /** The typed value: the plain value, or a new secret ("" = none typed). */
  value: string;
}

/** Context for {@link validateVariableForm}. */
export interface VariableFormContext {
  /** The variable being edited; undefined when adding. */
  existing?: Variable;
  /** All variables — for the case-insensitive uniqueness check. */
  variables: readonly Variable[];
  /** All forwards — a used variable can't change into a kind they can't use. */
  forwards: readonly ForwardConfig[];
}

export type VariableFormField = "name" | "secret" | "value";
export type VariableFieldErrors = Partial<Record<VariableFormField, string>>;

function listNames(uses: VariableUse[]): string {
  const names = uses.map((u) => u.forwardName);
  return names.length <= 2
    ? names.join(" and ")
    : `${names.slice(0, 2).join(", ")} and ${names.length - 2} more`;
}

/**
 * Validate the variable dialog. Mirrors the backend's rules so the dialog can
 * explain a refusal before the round trip; the backend still has the last say.
 */
export function validateVariableForm(
  values: VariableFormValues,
  ctx: VariableFormContext,
): VariableFieldErrors {
  const errors: VariableFieldErrors = {};
  const name = values.name.trim();

  if (!ctx.existing) {
    if (name === "") {
      errors.name = "Name is required.";
    } else if (!VARIABLE_NAME_RE.test(name)) {
      errors.name =
        "Use letters, digits and _, not starting with a digit (max 64).";
    } else {
      const clash = ctx.variables.find(
        (v) => v.name.toLowerCase() === name.toLowerCase(),
      );
      if (clash) {
        errors.name = `${clash.name} already exists — names ignore case.`;
      }
    }
  }

  if (ctx.existing && values.secret !== ctx.existing.secret) {
    const uses = variableUses(ctx.forwards, ctx.existing.name);
    if (values.secret) {
      const inUsername = uses.filter((u) => u.inUsername);
      if (inUsername.length > 0) {
        errors.secret = `Used in the username of ${listNames(inUsername)}, so it has to stay plain.`;
      }
    } else {
      const asPassword = uses.filter((u) => u.asPassword);
      if (asPassword.length > 0) {
        errors.secret = `It's the password of ${listNames(asPassword)}, so it has to stay secret.`;
      }
    }
  }

  if (values.secret) {
    // Blank keeps the stored secret — only possible when one is stored.
    const keeps =
      ctx.existing?.secret === true && ctx.existing.hasStoredValue === true;
    if (values.value === "" && !keeps) {
      errors.value = "Enter the secret value.";
    } else if (new TextEncoder().encode(values.value).length > SECRET_MAX) {
      errors.value = `The secret is longer than ${SECRET_MAX} bytes.`;
    }
  } else if (values.value === "") {
    errors.value = "Value is required.";
  } else if ([...values.value].length > PLAIN_VALUE_MAX) {
    errors.value = `Keep the value to ${PLAIN_VALUE_MAX} characters or fewer.`;
  } else if (values.value.includes("{{")) {
    errors.value = "A value can't contain {{ — variables don't nest.";
  } else if (
    /[\p{Cc}\u00AD\u061C\u180E\u200B-\u200F\u202A-\u202E\u2060-\u2064\u2066-\u2069\uFEFF\u034F\u115F\u1160\u2028\u2029\u3164\uFFA0\u{E0000}-\u{E007F}]/u.test(
      values.value,
    )
  ) {
    errors.value =
      "The value contains a control or invisible formatting character.";
  } else if (values.value.trim().startsWith("-")) {
    errors.value =
      "A value can't start with “-” (ssh would read it as an option).";
  }

  return errors;
}
