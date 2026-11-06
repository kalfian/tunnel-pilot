// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import LoginScreen from "./LoginScreen.svelte";
import type { SignInResult } from "../auth";

function setup(
  result: SignInResult,
  props: Partial<{ insecure: boolean; sessionEnded: boolean }> = {},
) {
  const onSignIn = vi.fn<(password: string) => Promise<SignInResult>>(() =>
    Promise.resolve(result),
  );
  render(LoginScreen, { host: "10.0.0.5:1855", onSignIn, ...props });
  const field = screen.getByLabelText("Admin password") as HTMLInputElement;
  const submit = screen.getByRole("button", { name: "Sign in" });
  return { onSignIn, field, submit };
}

describe("LoginScreen", () => {
  it("names the product and the server it signs in to", () => {
    setup({ ok: true });
    expect(
      screen.getByRole("heading", { name: "Tunnel Pilot" }),
    ).toBeInTheDocument();
    expect(screen.getByText("10.0.0.5:1855")).toBeInTheDocument();
  });

  it("focuses the password field, typed as a current password", () => {
    const { field } = setup({ ok: true });
    expect(field).toHaveFocus();
    expect(field).toHaveAttribute("type", "password");
    expect(field).toHaveAttribute("autocomplete", "current-password");
  });

  it("asks for a password instead of sending an empty one", async () => {
    const { onSignIn, submit, field } = setup({ ok: true });
    await fireEvent.click(submit);
    expect(onSignIn).not.toHaveBeenCalled();
    expect(field).toHaveAttribute("aria-invalid", "true");
    expect(field).toHaveAccessibleDescription("Enter the admin password.");
  });

  it("sends the password and reports a wrong one", async () => {
    const { onSignIn, submit, field } = setup({ ok: false, reason: "invalid" });
    await fireEvent.input(field, { target: { value: "hunter2" } });
    await fireEvent.click(submit);
    expect(onSignIn).toHaveBeenCalledWith("hunter2");
    // Visible under the field + spoken once via the live region.
    await vi.waitFor(() =>
      expect(field).toHaveAccessibleDescription("Incorrect password."),
    );
    await vi.waitFor(() =>
      expect(document.querySelector("[aria-live]")).toHaveTextContent(
        "Incorrect password.",
      ),
    );
  });

  it("explains the rate limit with the wait time", async () => {
    const { submit, field } = setup({
      ok: false,
      reason: "rateLimited",
      retryAfterSec: 240,
    });
    await fireEvent.input(field, { target: { value: "x" } });
    await fireEvent.click(submit);
    await vi.waitFor(() =>
      expect(field).toHaveAccessibleDescription(
        "Too many failed attempts. Try again in 4 minutes.",
      ),
    );
  });

  it("warns about plain HTTP and ties the warning to the field", () => {
    const { field } = setup({ ok: true }, { insecure: true });
    expect(screen.getByRole("note")).toHaveTextContent(/not encrypted/i);
    expect(field).toHaveAccessibleDescription(/admin password/i);
  });

  it("says why it is showing after a session ended", () => {
    setup({ ok: true }, { sessionEnded: true });
    expect(screen.getByText(/your session ended/i)).toBeInTheDocument();
  });
});
