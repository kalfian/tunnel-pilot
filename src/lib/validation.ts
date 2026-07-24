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
 */

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
  if (hasPassword && hasIdentity) {
    errors.auth = "Use either a password or an identity file, not both.";
  }

  return errors;
}

/** True when the given errors map is empty. */
export function isFormValid(errors: FieldErrors): boolean {
  return Object.keys(errors).length === 0;
}
