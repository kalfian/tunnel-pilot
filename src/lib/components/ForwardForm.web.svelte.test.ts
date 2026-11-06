// @vitest-environment jsdom
/** Web mode: the identity file is a typed SERVER path — no native picker. */
import "@testing-library/jest-dom/vitest";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/svelte";

vi.mock("../runtime", () => ({ isWeb: true }));
vi.mock("../ipc", () => ({
  createForward: vi.fn(),
  updateForward: vi.fn(),
  setForwardPassword: vi.fn(),
  clearForwardPassword: vi.fn(),
}));

import ForwardForm from "./ForwardForm.svelte";

describe("ForwardForm — web mode", () => {
  it("takes a server path for the identity file, with no Browse button", async () => {
    render(ForwardForm, { props: { mode: "add", onClose: vi.fn() } });
    const auth = screen.getByRole("radiogroup", {
      name: /authentication method/i,
    });
    await fireEvent.click(
      within(auth).getByRole("radio", { name: /identity file/i }),
    );

    const field = screen.getByLabelText(/^identity file$/i);
    expect(field).toHaveAccessibleDescription(/on the server/i);
    expect(
      screen.queryByRole("button", { name: /browse/i }),
    ).not.toBeInTheDocument();
  });
});
