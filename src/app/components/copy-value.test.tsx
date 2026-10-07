import { screen } from "@testing-library/react";
import { renderWithProviders } from "@tests/test-utils";
import { CopyValue } from "./copy-value";

describe("CopyValue (D-093)", () => {
  it("copies on click and says so", async () => {
    const { user } = renderWithProviders(
      <CopyValue value="192.168.1.24" label="local IP address" />
    );
    const write = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue(undefined);
    await user.click(
      screen.getByRole("button", { name: "Copy local IP address 192.168.1.24" })
    );
    expect(write).toHaveBeenCalledWith("192.168.1.24");
    expect((await screen.findAllByText("Copied"))[0]).toBeInTheDocument();
  });

  it("says when the clipboard refused", async () => {
    const { user } = renderWithProviders(
      <CopyValue value="203.0.113.42" label="public IP address" />
    );
    vi.spyOn(navigator.clipboard, "writeText").mockRejectedValue(
      new Error("denied")
    );
    vi.spyOn(console, "error").mockImplementation(() => {});
    await user.click(
      screen.getByRole("button", {
        name: "Copy public IP address 203.0.113.42",
      })
    );
    expect(
      (await screen.findAllByText("Couldn't copy"))[0]
    ).toBeInTheDocument();
  });
});
