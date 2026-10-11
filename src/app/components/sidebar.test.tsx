import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { Sidebar, type SidebarEntry } from "./sidebar";

it("keeps values, route selection, navigation handlers and keyboard order current when groups change", async () => {
  const user = userEvent.setup();
  const navigate = vi.fn();
  const nextNavigate = vi.fn();
  const nav: readonly SidebarEntry[] = [
    {
      id: "overview",
      href: "/dashboard/overview",
      label: "Overview",
      icon: "overview",
    },
  ];
  const tools: readonly SidebarEntry[] = [
    {
      id: "containers",
      href: "/dashboard/containers",
      label: "Containers",
      icon: "containers",
    },
  ];
  const props = (
    value: string,
    local: boolean,
    active: string,
    onNavigate: typeof navigate
  ) => ({
    groups: [
      { id: "nav", entries: nav },
      {
        id: "modules",
        entries: [
          {
            id: "cpu",
            href: "/dashboard/cpu",
            label: "CPU",
            icon: "cpu" as const,
            value,
          },
        ],
      },
      { id: "tools", entries: local ? tools : [] },
    ],
    active,
    onNavigate,
    status: { intervalMs: 1000, paused: false, onBattery: false },
    version: "test",
  });
  const view = render(
    <Sidebar {...props("10%", true, "overview", navigate)} />
  );
  expect(screen.getByRole("link", { name: "Overview" })).toHaveAttribute(
    "aria-current",
    "page"
  );
  await user.click(screen.getByRole("link", { name: "CPU 10%" }));
  expect(navigate).toHaveBeenLastCalledWith("/dashboard/cpu");

  view.rerender(<Sidebar {...props("10%", true, "cpu", navigate)} />);
  expect(screen.getByRole("link", { name: "Overview" })).not.toHaveAttribute(
    "aria-current"
  );
  expect(screen.getByRole("link", { name: "CPU 10%" })).toHaveAttribute(
    "aria-current",
    "page"
  );
  view.rerender(<Sidebar {...props("20%", true, "cpu", navigate)} />);
  expect(screen.getByRole("link", { name: "CPU 20%" })).toBeVisible();
  view.rerender(<Sidebar {...props("20%", false, "cpu", navigate)} />);
  expect(screen.queryByRole("link", { name: "Containers" })).toBeNull();

  view.rerender(<Sidebar {...props("40%", false, "cpu", nextNavigate)} />);
  expect(screen.getByRole("link", { name: "CPU 40%" })).toHaveAttribute(
    "aria-current",
    "page"
  );
  expect(screen.getByRole("link", { name: "Overview" })).not.toHaveAttribute(
    "aria-current"
  );
  expect(screen.queryByRole("link", { name: "Containers" })).toBeNull();
  await user.click(screen.getByRole("link", { name: "Overview" }));
  expect(nextNavigate).toHaveBeenLastCalledWith("/dashboard/overview");
  await user.keyboard("{End}");
  expect(screen.getByRole("link", { name: "CPU 40%" })).toHaveFocus();

  view.rerender(<Sidebar {...props("60%", true, "cpu", nextNavigate)} />);
  await user.keyboard("{End}");
  expect(screen.getByRole("link", { name: "Containers" })).toHaveFocus();
  await user.keyboard("{Enter}");
  expect(nextNavigate).toHaveBeenLastCalledWith("/dashboard/containers");
  await user.keyboard("{Home}{ArrowDown}");
  expect(screen.getByRole("link", { name: "CPU 60%" })).toHaveFocus();
  const latestNavigate = vi.fn();
  view.rerender(<Sidebar {...props("60%", true, "cpu", latestNavigate)} />);
  await user.click(screen.getByRole("link", { name: "Overview" }));
  expect(latestNavigate).toHaveBeenLastCalledWith("/dashboard/overview");
});
